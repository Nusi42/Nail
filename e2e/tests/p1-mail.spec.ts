import { expect, test, type Browser, type FrameLocator, type Page } from "@playwright/test";
import { connectDesktop, skipFirstRun, startDesktop, type DesktopApp } from "../support/desktop";
import { control, freePort, startNail, type NailServer } from "../support/nail";
import {
  createUser,
  enableRemoteUi,
  pairBrowser,
  pairingCode,
  remoteContext,
  rpcOk,
} from "../support/remote";
import { expectResponsive } from "../support/responsive";
import { READER_SUBJECT, showView } from "../support/views";

/**
 * P1 WP190: HTML mail renders as its author designed it (style sheets,
 * classes, body colours, inline `cid:` images, remote images after
 * allowing them) in `nail serve` (local UI and the remote UI's sandbox
 * origin) and in the desktop app, and the mail view passes the P1
 * responsive standard (`e2e/support/responsive.ts`). Every assertion about
 * the mail looks inside the reader frame (computed styles, loaded images).
 */

/** A valid 1×1 PNG (the corpus's lab bench photo). */
const PNG_1X1 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

/** A newsletter as real senders write it: a style sheet in <head>, classes, body bgcolor, a cid: logo and a remote banner. */
function newsletter(remoteImage: string): string {
  return [
    "From: Shop <news@shop.example>",
    "To: ada@work.example",
    "Subject: Big October Sale",
    "Message-ID: <sale@shop.example>",
    "Date: Mon, 05 Oct 2026 09:00:00 +0000",
    "MIME-Version: 1.0",
    'Content-Type: multipart/related; boundary="rel"',
    "",
    "--rel",
    "Content-Type: text/html; charset=utf-8",
    "",
    '<!doctype html><html><head><title>Sale</title><style type="text/css">',
    "  .hero { background-color: #0b7285; color: #ffffff; padding: 24px; }",
    "  .btn { display: inline-block; background-color: #e8590c; color: #ffffff; padding: 8px 16px; }",
    "  @media (max-width: 480px) { .hero { padding: 8px; } }",
    "</style></head>",
    '<body bgcolor="#f4f4f4"><table class="card" align="center" width="600" cellpadding="0" cellspacing="0">',
    '<tr><td class="hero"><h1>Big October Sale</h1><p>Everything 30% off this week.</p></td></tr>',
    `<tr><td><img alt="Shop logo" src="cid:logo@shop.example" width="40" height="40"> <img alt="Banner" src="${remoteImage}" width="200" height="50">`,
    '<a class="btn" href="https://shop.example/">Shop now</a></td></tr></table></body></html>',
    "--rel",
    "Content-Type: image/png",
    "Content-ID: <logo@shop.example>",
    "Content-Transfer-Encoding: base64",
    'Content-Disposition: inline; filename="logo.png"',
    "",
    PNG_1X1,
    "--rel--",
    "",
  ].join("\n");
}

/** An HTML-only message, `html` its body. */
function htmlMail(subject: string, id: string, html: string): string {
  return [
    "From: Tea Club <hello@tea.example>",
    "To: ada@work.example",
    `Subject: ${subject}`,
    `Message-ID: <${id}@tea.example>`,
    "Date: Mon, 05 Oct 2026 08:00:00 +0000",
    "MIME-Version: 1.0",
    "Content-Type: text/html; charset=utf-8",
    "",
    html,
    "",
  ].join("\n");
}

const readerFrame = (page: Page): FrameLocator =>
  page.frameLocator('iframe[data-part="message-view.body"]');

/** Opens the message whose list row shows `subject` (in the Mail view). */
async function openMessage(page: Page, subject: string): Promise<void> {
  const row = page.locator('[data-part="message-list.row"]', { hasText: subject }).first();
  await expect(row).toBeVisible({ timeout: 30_000 });
  await row.click();
  await expect(page.locator(READER_SUBJECT)).toHaveText(subject);
}

/** A computed style property of the first element matching `selector` in the frame. */
function frameStyle(frame: FrameLocator, selector: string, property: string): Promise<string> {
  return frame
    .locator(selector)
    .first()
    .evaluate((el, prop) => getComputedStyle(el).getPropertyValue(prop), property);
}

/**
 * The natural width of the image with `alt` in the frame (0 = not loaded,
 * -1 = the frame was just replaced, e.g. after "Load once"; polled again).
 */
function naturalWidth(frame: FrameLocator, alt: string): Promise<number> {
  return frame
    .getByAltText(alt)
    .evaluate((img: HTMLImageElement) => img.naturalWidth)
    .catch(() => -1);
}

/**
 * The reader lists no attachment card for the message's only attachment, an
 * image its body shows (ADR 0162); "Show all attachments (1)" reveals the
 * card (`name`), still downloadable, and hides it again.
 */
async function expectEmbeddedOnly(page: Page, name: string): Promise<void> {
  const reader = page.locator('[data-part="message-view"]');
  const toggle = reader.locator('[data-part="message-view.attachments.toggle"]');
  await expect(toggle).toHaveText("Show all attachments (1)");
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await expect(reader.locator('[data-part="attachment-card"]')).toHaveCount(0);
  await expect(reader.getByText("Unnamed attachment")).toHaveCount(0);
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  await expect(toggle).toHaveText("Hide images shown in the message");
  const cards = reader.getByRole("list", { name: "Attachments" }).getByRole("listitem");
  await expect(cards).toHaveCount(1);
  await expect(cards.first()).toContainText(name);
  await expect(cards.first().getByRole("button", { name: `Save ${name}` })).toBeEnabled();
  await toggle.click();
  await expect(reader.locator('[data-part="attachment-card"]')).toHaveCount(0);
}

interface RemoteHosts {
  images: string;
  private: string;
  requests: string[];
}

test.describe("HTML mail in nail serve (demo-multi)", () => {
  let server: NailServer;

  test.beforeEach(async () => {
    server = await startNail({ seed: "demo-multi" });
  });

  test.afterEach(async () => {
    await server.stop();
  });

  test("a newsletter keeps its style sheet, classes, body colour and inline image; remote images wait for the user", async ({
    page,
  }) => {
    const hosts = await control<RemoteHosts>(server, "remote-content");
    await control(server, "imap-deliver", { raw: newsletter(`${hosts.images}/logo.png`) });
    await page.goto(server.url);
    await showView(page, "Mail");
    // The list preview keeps the blocks apart (not "Big October SaleEverything").
    const row = page.locator('[data-part="message-list.row"]', { hasText: "Big October Sale" });
    await expect(row).toContainText("Big October Sale Everything 30% off this week.", {
      timeout: 30_000,
    });
    await openMessage(page, "Big October Sale");

    const body = page.locator('iframe[data-part="message-view.body"]');
    // P3 (ADR 0164): the app's origin for measuring, never scripts.
    await expect(body).toHaveAttribute("sandbox", "allow-same-origin");
    await expect(body).toHaveAttribute("data-state", /(^|\s)designed(\s|$)/);
    const frame = readerFrame(page);
    await expect(frame.getByRole("heading", { name: "Big October Sale" })).toBeVisible();
    // The style sheet and the classes arrived: computed colours inside the frame.
    expect(await frameStyle(frame, ".hero", "background-color")).toBe("rgb(11, 114, 133)");
    expect(await frameStyle(frame, ".hero", "color")).toBe("rgb(255, 255, 255)");
    expect(await frameStyle(frame, ".btn", "background-color")).toBe("rgb(232, 89, 12)");
    expect(await frameStyle(frame, "body", "background-color")).toBe("rgb(244, 244, 244)");
    // The 600 px layout table is not squeezed to the reading width.
    const tableWidth = await frame
      .locator("table.card")
      .evaluate((t) => t.getBoundingClientRect().width);
    expect(tableWidth).toBeGreaterThanOrEqual(599);
    // The inline (cid:) logo shows, embedded by the core.
    await expect.poll(() => naturalWidth(frame, "Shop logo"), { timeout: 15_000 }).toBe(1);
    expect(await frame.getByAltText("Shop logo").getAttribute("src")).toMatch(
      /^data:image\/png;base64,/,
    );
    // ADR 0162: the logo the body shows is no attachment card; the toggle lists it.
    await expectEmbeddedOnly(page, "logo.png");
    // The remote banner is blocked: no address at all, a quiet placeholder.
    const banner = frame.getByAltText("Banner");
    expect(await banner.getAttribute("src")).toBeNull();
    expect(
      await banner.evaluate((img) => getComputedStyle(img, "::after").getPropertyValue("content")),
    ).toBe('"Banner"');
    const bar = page.getByRole("region", { name: "Remote content" });
    await expect(bar).toContainText("Nail blocked 1");

    // "Load once": the banner arrives through the core's proxy.
    await bar.getByRole("button", { name: "Load once" }).click();
    await expect.poll(() => naturalWidth(frame, "Banner"), { timeout: 15_000 }).toBe(1);
    // Loaded, it keeps the size its attributes give (200 × 50), not its own proportions (1 × 1).
    expect(
      await frame.getByAltText("Banner").evaluate((img) => {
        const box = img.getBoundingClientRect();
        return [Math.round(box.width), Math.round(box.height)];
      }),
    ).toEqual([200, 50]);
    expect(await frameStyle(frame, ".hero", "background-color")).toBe("rgb(11, 114, 133)");
    const requested = await control<RemoteHosts>(server, "remote-content");
    expect(requested.requests).toContain("/logo.png");
  });

  test("in a dark theme designed mail keeps its colours on paper, plain mail takes the theme's", async ({
    page,
  }) => {
    await control(server, "imap-deliver", {
      raw: htmlMail(
        "Red letter day",
        "red",
        '<style>.alert { color: #c92a2a; font-weight: bold; }</style><p class="alert">Red letters</p><p>Black letters and <a href="https://tea.example/guide">a guide</a></p>',
      ),
    });
    await control(server, "imap-deliver", {
      raw: htmlMail("Plain greetings", "plain", "<p>Hello <b>Ada</b>, plain words.</p>"),
    });
    await rpcOk(server, { type: "SetTheme", theme_id: "graphite" }, "Themes");
    await page.goto(server.url);
    await showView(page, "Mail");
    await expect(page.locator("html")).toHaveAttribute("data-nail-mode", "dark");

    await openMessage(page, "Red letter day");
    const frame = readerFrame(page);
    await expect(frame.getByText("Red letters")).toBeVisible();
    expect(await frameStyle(frame, "body", "background-color")).toBe("rgb(255, 255, 255)");
    expect(await frameStyle(frame, ".alert", "color")).toBe("rgb(201, 42, 42)");
    // Graphite's print colours: black text, a dark link colour readable on
    // the white paper (not the dark scheme's pale LinkText), and a light
    // colour scheme for the frame (scrollbars, controls).
    expect(await frameStyle(frame, "p:not(.alert)", "color")).toBe("rgb(0, 0, 0)");
    expect(await frameStyle(frame, "a", "color")).toBe("rgb(59, 91, 219)");
    expect(await frameStyle(frame, "html", "color-scheme")).toBe("light");

    await openMessage(page, "Plain greetings");
    await expect(frame.getByText("plain words")).toBeVisible();
    const background = await frameStyle(frame, "body", "background-color");
    const text = await frameStyle(frame, "p", "color");
    expect(background).not.toBe("rgb(255, 255, 255)");
    expect(await frameStyle(frame, "html", "color-scheme")).toBe("dark");
    // Light text on the dark theme background.
    expect(luminance(text)).toBeGreaterThan(luminance(background));
    // Plain mail keeps the reading width.
    const width = await frame.locator("p").evaluate((p) => getComputedStyle(p).maxWidth);
    expect(width).not.toBe("none");
  });
});

/** Relative luminance-ish brightness of an `rgb(…)` value (enough to order two colours). */
function luminance(rgb: string): number {
  const [r = 0, g = 0, b = 0] = (rgb.match(/\d+/g) ?? []).map(Number);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

test.describe("HTML mail from the demo corpus", () => {
  let server: NailServer;

  test.beforeEach(async () => {
    server = await startNail({ seed: "demo" });
  });

  test.afterEach(async () => {
    await server.stop();
  });

  test("the corpus's inline image, newsletter and hostile mail render in the frame", async ({
    page,
  }) => {
    await page.goto(server.url);
    await showView(page, "Mail");
    const frame = readerFrame(page);

    await openMessage(page, "Photo of the new lab bench");
    await expect(frame.getByText("Here is the new bench")).toBeVisible();
    await expect.poll(() => naturalWidth(frame, "Lab bench"), { timeout: 15_000 }).toBe(1);
    await expectEmbeddedOnly(page, "bench.png");

    await openMessage(page, "Your weekly garden digest 🌱");
    await expect(frame.getByRole("heading", { name: "Autumn is here" })).toBeVisible();
    // The <body bgcolor> survives as a body rule; the remote style is gone.
    expect(await frameStyle(frame, "body", "background-color")).toBe("rgb(244, 241, 232)");
    expect(await frameStyle(frame, "td", "color")).toBe("rgb(45, 74, 34)");
    expect(await frame.getByAltText("Pumpkins").getAttribute("src")).toBeNull();

    await openMessage(page, "Action required: verify your mailbox");
    await expect(frame.getByText("Overlay")).toBeVisible();
    expect(await frameStyle(frame, "p:has-text('Overlay')", "position")).not.toBe("fixed");
  });

  test("the remote UI renders the same mail from its sandbox origin", async ({ page }) => {
    const remote = await enableRemoteUi(server);
    const user = await createUser(server, "ada");
    const context = await remoteContext(page);
    const phone = await context.newPage();
    await phone.setViewportSize({ width: 1280, height: 1000 });
    await pairBrowser(phone, remote, await pairingCode(server, user));
    await phone.getByRole("button", { name: "Not now" }).click();
    await showView(phone, "Mail");

    await openMessage(phone, "Photo of the new lab bench");
    const body = phone.locator('iframe[data-part="message-view.body"]');
    // Framed from the sandbox origin (another port), not as srcdoc.
    const src = (await body.getAttribute("src")) ?? "";
    expect(new URL(src).port).not.toBe(String(remote.port));
    expect(await body.getAttribute("srcdoc")).toBeNull();
    const frame = readerFrame(phone);
    await expect(frame.getByText("Here is the new bench")).toBeVisible();
    await expect.poll(() => naturalWidth(frame, "Lab bench"), { timeout: 15_000 }).toBe(1);

    await openMessage(phone, "Your weekly garden digest 🌱");
    await expect(frame.getByRole("heading", { name: "Autumn is here" })).toBeVisible();
    expect(await frameStyle(frame, "body", "background-color")).toBe("rgb(244, 241, 232)");
    await context.close();
  });
});

test.describe("the mail view at every standard size", () => {
  let server: NailServer;

  test.beforeEach(async () => {
    server = await startNail({ seed: "demo-multi" });
  });

  test.afterEach(async () => {
    await server.stop();
  });

  test("folder pane, list, chips, reader, toolbar and attachments stay in shape", async ({
    page,
  }) => {
    test.setTimeout(240_000);
    const hosts = await control<RemoteHosts>(server, "remote-content");
    await control(server, "imap-deliver", { raw: newsletter(`${hosts.images}/logo.png`) });
    await page.goto(server.url);
    await showView(page, "Mail");
    const mail = '[aria-label="Mail view"]';

    // Below 900 px (`defaults/layout.kdl`) the mail window shows one pane at a
    // time behind tabs (Folders / Messages / Message).
    const showPane = async (name: "Folders" | "Messages" | "Message") => {
      // Toolbars re-fit after a resize (ResizeObserver → measure → fit, a
      // frame or two later): wait until none is measuring or overflowing.
      await expect
        .poll(() =>
          page.evaluate(() =>
            [...document.querySelectorAll<HTMLElement>('[data-part="toolbar"]')]
              .filter((bar) => bar.getClientRects().length > 0)
              .filter(
                (bar) => bar.dataset.state === "measuring" || bar.scrollWidth > bar.clientWidth + 1,
              )
              .map(
                (bar) =>
                  `${bar.getAttribute("aria-label") ?? ""} ${String(bar.scrollWidth)}>${String(bar.clientWidth)}`,
              ),
          ),
        )
        .toEqual([]);
      const viewport = page.viewportSize();
      if (viewport === null || viewport.width >= 900) return;
      const tab = page
        .getByRole("region", { name: "Mail view" })
        .getByRole("tab", { name, exact: true });
      await tab.click();
      await expect(tab).toHaveAttribute("aria-selected", "true");
    };

    // A designed HTML mail with an inline image (an attachment card) and the remote-content bar.
    await openMessage(page, "Big October Sale");
    await expectResponsive(page, "p1-mail-newsletter", {
      scope: mail,
      prepare: async () => {
        await showPane("Message");
        await expect(
          readerFrame(page).getByRole("heading", { name: "Big October Sale" }),
        ).toBeVisible();
      },
    });

    // A plain-text message with two attachment cards.
    await showPane("Messages");
    await openMessage(page, "Your invoice for September 2026");
    await expectResponsive(page, "p1-mail-attachments", {
      scope: mail,
      prepare: async () => {
        await showPane("Message");
        await expect(page.locator('[data-part="attachment-card"]').first()).toBeVisible();
      },
    });

    // The list with its header chips, and the folder pane.
    await expectResponsive(page, "p1-mail-list", {
      scope: mail,
      prepare: async () => {
        await showPane("Messages");
        await expect(page.locator('[data-part="list-header.chips"]')).toBeVisible();
      },
    });
    await expectResponsive(page, "p1-mail-folders", {
      scope: mail,
      prepare: async () => {
        await showPane("Folders");
        await expect(page.locator('[data-part="folder-tree.compose"]')).toBeVisible();
      },
    });
    await showPane("Messages");

    // The page never scrolls with a long message either: the reader's card
    // feed scrolls (P3, ADR 0164: the frame is as tall as the whole mail, so
    // it may reach below the pane), never the page.
    await page.setViewportSize({ width: 1280, height: 700 });
    await openMessage(page, "Big October Sale");
    const frame = page.locator('iframe[data-part="message-view.body"]');
    await expect(frame).toHaveAttribute("data-state", /(^|\s)measured(\s|$)/);
    await expect
      .poll(async () => {
        const content = await frame
          .contentFrame()
          .locator("html")
          .evaluate((root: HTMLElement) => root.offsetHeight);
        return Math.abs(((await frame.boundingBox())?.height ?? 0) - content);
      })
      .toBeLessThanOrEqual(2);
    const pageScroll = await page.evaluate(() => ({
      scroll: document.documentElement.scrollHeight,
      height: window.innerHeight,
    }));
    expect(pageScroll.scroll).toBeLessThanOrEqual(pageScroll.height + 1);
    const feedScrolls = await page
      .locator('[data-part="thread-view"]')
      .evaluate((el) => getComputedStyle(el).overflowY);
    expect(["auto", "scroll"]).toContain(feedScrolls);
  });
});

test.describe("HTML mail in the desktop app", () => {
  test.skip(process.platform !== "win32", "the desktop app uses WebView2 (Windows)");
  test.skip(
    !!process.env.CI && !process.env.NAIL_E2E_DESKTOP,
    "set NAIL_E2E_DESKTOP=1 to run the desktop tests in CI",
  );
  test.describe.configure({ mode: "serial", timeout: 180_000 });

  let app: DesktopApp;
  let browser: Browser;
  let page: Page;

  test.beforeAll(async () => {
    app = await startDesktop(freePort());
    ({ browser, page } = await connectDesktop(app));
    await skipFirstRun(page);
  });

  test.afterAll(async () => {
    await (browser as Browser | undefined)?.close().catch(() => undefined);
    await (app as DesktopApp | undefined)?.stop();
  });

  test("the desktop reader shows inline images and the newsletter's colours", async () => {
    await showView(page, "Mail");
    const frame = readerFrame(page);
    await openMessage(page, "Photo of the new lab bench");
    await expect(frame.getByText("Here is the new bench")).toBeVisible();
    await expect.poll(() => naturalWidth(frame, "Lab bench"), { timeout: 15_000 }).toBe(1);
    await openMessage(page, "Your weekly garden digest 🌱");
    await expect(frame.getByRole("heading", { name: "Autumn is here" })).toBeVisible();
    expect(await frameStyle(frame, "body", "background-color")).toBe("rgb(244, 241, 232)");
  });
});
