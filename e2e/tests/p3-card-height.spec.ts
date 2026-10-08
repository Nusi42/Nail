import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { desktopTests, desktopPerFile } from "../support/desktop";
import { buildOutputDir, control, nailOnDemand } from "../support/nail";
import { rpcOk, openInDarkTheme, pairedRemoteMail } from "../support/remote";
import { showView, messageRow } from "../support/views";

/**
 * P3 WP-card-height (ADR 0164): HTML mail frames are as tall as their mail —
 * a short HTML card is short, a long newsletter shows in full and the feed
 * (or the single-message reader) scrolls as a whole; the frames measure
 * again after remote content loads, when their width changes and after a
 * theme switch. Local frames (`srcdoc`, `sandbox="allow-same-origin"`) are
 * measured by the reader; remote UI frames on the sandbox origin
 * (`sandbox="allow-scripts"`, opaque origin) report through the route's
 * hash-pinned script. No mail script, handler, `javascript:` link, meta
 * refresh or CSS expression runs on either path; Trusted Types stay
 * enforced in the remote UI. The desktop app (WebView2) measures its local
 * frames like the browser.
 *
 * Screenshots (owner, phone): `<build dir>/screens/p3/card-height-*.png`,
 * Frost (light) and Graphite / Nocturne (dark).
 */

const server = nailOnDemand();

/** A fresh `nail serve --seed demo-multi` for each test of the calling describe block. */
function useServer(): void {
  test.beforeEach(async () => {
    await server.start({ seed: "demo-multi" });
  });
}

interface RemoteHosts {
  images: string;
  requests: string[];
}

const SUBJECT = "Autumn reading list";

/** A message as the mock IMAP server delivers it. */
function mail(options: {
  from: string;
  subject: string;
  id: string;
  date: string;
  html: string;
  inReplyTo?: string;
}): string {
  return [
    `From: ${options.from}`,
    "To: ada@work.example",
    `Subject: ${options.subject}`,
    `Message-ID: <${options.id}@club.example>`,
    ...(options.inReplyTo
      ? [
          `In-Reply-To: <${options.inReplyTo}@club.example>`,
          `References: <${options.inReplyTo}@club.example>`,
        ]
      : []),
    `Date: ${options.date}`,
    "MIME-Version: 1.0",
    "Content-Type: text/html; charset=utf-8",
    "",
    options.html,
    "",
  ].join("\n");
}

/** A long newsletter: a 600 px layout table (designed mail) and many paragraphs; `banner` = a remote image. */
function newsletterHtml(banner: string | null): string {
  const items = Array.from(
    { length: 36 },
    (_, i) =>
      `<tr><td><h2>Book ${String(i + 1)}</h2><p>A long description of book ${String(i + 1)}, ` +
      "with enough words to wrap over a few lines in the newsletter's column, as real ones do " +
      "when they introduce the season's reading.</p></td></tr>",
  ).join("");
  const image =
    banner === null ? "" : `<tr><td><img alt="Banner" width="560" src="${banner}"></td></tr>`;
  return `<table width="600" class="news">${image}<tr><td><h1>The autumn list</h1></td></tr>${items}<tr><td><p>Last line of the list.</p></td></tr></table>`;
}

/** The conversation: the long newsletter, then a short HTML reply. */
async function deliverConversation(banner: string | null = null): Promise<void> {
  await control(server, "imap-deliver", {
    raw: mail({
      from: "Book Club <club@club.example>",
      subject: SUBJECT,
      id: "list",
      date: "Mon, 05 Oct 2026 08:00:00 +0000",
      html: newsletterHtml(banner),
    }),
  });
  await control(server, "imap-deliver", {
    raw: mail({
      from: "Grace Hopper <grace@club.example>",
      subject: `Re: ${SUBJECT}`,
      id: "reply",
      date: "Mon, 05 Oct 2026 09:00:00 +0000",
      inReplyTo: "list",
      html: "<p>Thanks, <b>see you Monday</b>.</p>",
    }),
  });
}

const rowOf = (page: Page, subject: string) => messageRow(page, subject).first();
const cards = (page: Page) => page.getByRole("feed").locator('[data-part="thread-view.card"]');
const frameIn = (scope: Locator) => scope.locator('iframe[data-part="message-view.body"]');

/** Opens the conversation and expands every card. */
async function openConversation(page: Page): Promise<{ long: Locator; short: Locator }> {
  const row = rowOf(page, SUBJECT);
  await expect(row).toBeVisible({ timeout: 30_000 });
  await row.click();
  await expect(page.locator('[data-part="thread-view.subject"]')).toContainText(SUBJECT);
  await expect(cards(page)).toHaveCount(2);
  const expandAll = page.getByRole("button", { name: "Expand all" });
  if (await expandAll.isVisible()) await expandAll.click();
  const long = cards(page).filter({ hasText: "Book Club" });
  const short = cards(page).filter({ hasText: "Grace Hopper" });
  await expect(frameIn(long)).toBeVisible();
  await expect(frameIn(short)).toBeVisible();
  return { long, short };
}

interface Fit {
  /** The frame element's height in the page. */
  frame: number;
  /** The document's viewport (`clientHeight`) and content (`scrollHeight`, `offsetHeight`). */
  client: number;
  scroll: number;
  offset: number;
  /** The horizontal scrollbar's thickness, 0 without one. */
  bar: number;
}

/** How the frame's height compares with its document (works across origins). */
async function fitOf(frame: Locator): Promise<Fit> {
  const box = await frame.boundingBox();
  const inner = await frame
    .contentFrame()
    .locator("html")
    .evaluate((root: HTMLElement) => ({
      client: root.clientHeight,
      scroll: root.scrollHeight,
      offset: root.offsetHeight,
      // A horizontal scrollbar (wide mail) sits between the viewport's and the root's bottom.
      bar: Math.max(0, window.innerHeight - root.clientHeight),
    }));
  return { frame: box?.height ?? 0, ...inner };
}

/** Waits until `frame` is measured and shows its whole document: no inner vertical scroll. */
async function expectFitted(frame: Locator): Promise<Fit> {
  await expect(frame).toHaveAttribute("data-state", /(^|\s)measured(\s|$)/);
  await expect
    .poll(
      async () => {
        const fit = await fitOf(frame);
        // The document fits (nothing below the frame's edge) and the frame
        // is not taller than its content (no empty space).
        return fit.scroll <= fit.client + 1 && Math.abs(fit.frame - fit.offset - fit.bar) <= 2;
      },
      { timeout: 15_000 },
    )
    .toBe(true);
  return fitOf(frame);
}

function screenshotPath(name: string): string {
  const dir = buildOutputDir(path.join("screens", "p3"));
  mkdirSync(dir, { recursive: true });
  return path.join(dir, `card-height-${name}.png`);
}

/** A document that would act if anything in it ran: a script, a handler, a `javascript:` link, a meta refresh, a CSS expression. */
const HOSTILE =
  '<meta http-equiv="refresh" content="0;url=https://evil.example/">' +
  "<p>Hostile words</p>" +
  '<script>parent.postMessage({type:"nail-frame-height",height:4321},"*");document.documentElement.dataset.pwned="script";try{parent.document.documentElement.dataset.pwned="script"}catch(e){}</script>' +
  '<img alt="x" src="data:," onerror="document.documentElement.dataset.pwned=\'handler\';try{parent.document.documentElement.dataset.pwned=\'handler\'}catch(e){}">' +
  "<a id=\"js\" href=\"javascript:document.documentElement.dataset.pwned='link';try{parent.document.documentElement.dataset.pwned='link'}catch(e){}\">Click me</a>" +
  '<div style="width: expression(alert(1)); background: url(javascript:alert(1))">Styled</div>';

/** Asserts that nothing in the reader frame `frame` ran (inside and in the app). */
async function expectInert(page: Page, frame: Locator): Promise<void> {
  const inner = frame.contentFrame();
  await expect(inner.getByText("Hostile words")).toBeVisible();
  const link = inner.locator("a", { hasText: "Click me" });
  if ((await link.count()) > 0) await link.click();
  // Time for a refresh, a handler or a posted height to act.
  await page.waitForTimeout(1500);
  await expect(inner.getByText("Hostile words")).toBeVisible();
  expect(await inner.locator("html").getAttribute("data-pwned")).toBeNull();
  expect(await page.locator("html").getAttribute("data-pwned")).toBeNull();
  // No posted height reached the reader (a forged 4321 px).
  expect((await frame.boundingBox())?.height ?? 0).toBeLessThan(4000);
}

test.describe("local frames (nail serve)", () => {
  useServer();

  test("a short HTML card is short, a long newsletter shows in full, and the feed scrolls as a whole", async ({
    page,
  }) => {
    const hosts = await control<RemoteHosts>(server, "remote-content");
    await deliverConversation(`${hosts.images}/logo.png`);
    await page.goto(server.url);
    await showView(page, "Mail");
    const { long, short } = await openConversation(page);

    // Local frames: the app's origin for measuring, never scripts.
    await expect(frameIn(short)).toHaveAttribute("sandbox", "allow-same-origin");

    const shortFit = await expectFitted(frameIn(short));
    expect(shortFit.frame).toBeLessThan(120);
    expect((await short.boundingBox())?.height ?? 0).toBeLessThan(200);

    const longFit = await expectFitted(frameIn(long));
    expect(longFit.frame).toBeGreaterThan(2000);
    // The feed scrolls, not the frame: the last line is reached by scrolling the reader.
    const lastLine = frameIn(long).contentFrame().getByText("Last line of the list.");
    await lastLine.scrollIntoViewIfNeeded();
    await expect(lastLine).toBeInViewport();
    await page.screenshot({ path: screenshotPath("frost-local-1280x1000") });

    // Remote content arrives (Load once): the banner grows from nothing to
    // 560 px square, and the frame measures again.
    const before = longFit.frame;
    await page
      .getByRole("region", { name: "Remote content" })
      .getByRole("button", { name: "Load once" })
      .click();
    await expect
      .poll(
        () =>
          frameIn(long)
            .contentFrame()
            .getByAltText("Banner")
            .evaluate((img: HTMLImageElement) => img.naturalWidth),
        { timeout: 15_000 },
      )
      .toBe(1);
    await expect
      .poll(async () => (await fitOf(frameIn(long))).frame, { timeout: 15_000 })
      .toBeGreaterThan(before + 500);
    await expectFitted(frameIn(long));

    // A narrower window: the frames get narrower, the text wraps again, and
    // both frames still fit.
    const wide = (await frameIn(long).boundingBox())?.width ?? 0;
    await page.setViewportSize({ width: 900, height: 1000 });
    await expect
      .poll(async () => (await frameIn(long).boundingBox())?.width ?? 0)
      .toBeLessThan(wide - 50);
    await expectFitted(frameIn(short));
    await expectFitted(frameIn(long));
    expect((await short.boundingBox())?.height ?? 0).toBeLessThan(260);
  });

  test("the single-message reader has no empty space below a short HTML mail", async ({ page }) => {
    await control(server, "imap-deliver", {
      raw: mail({
        from: "Grace Hopper <grace@club.example>",
        subject: "Lunch on Friday?",
        id: "lunch",
        date: "Mon, 05 Oct 2026 10:00:00 +0000",
        html: "<p>Shall we have lunch on <i>Friday</i>?</p>",
      }),
    });
    await page.goto(server.url);
    await showView(page, "Mail");
    await rowOf(page, "Lunch on Friday?").click();
    const frame = page.locator('iframe[data-part="message-view.body"]');
    const fit = await expectFitted(frame);
    expect(fit.frame).toBeLessThan(200);
  });

  test("nothing in a mail runs: not through the sanitizer, and not even past it", async ({
    page,
  }) => {
    await control(server, "imap-deliver", {
      raw: mail({
        from: "Mallory <mallory@club.example>",
        subject: "Hostile mail",
        id: "hostile",
        date: "Mon, 05 Oct 2026 11:00:00 +0000",
        html: HOSTILE,
      }),
    });
    await page.goto(server.url);
    await showView(page, "Mail");
    await rowOf(page, "Hostile mail").click();
    const frame = page.locator('iframe[data-part="message-view.body"]');
    await expect(frame).toHaveAttribute("sandbox", "allow-same-origin");
    await expectInert(page, frame);

    // Past the sanitizer: the raw document straight into the reader's frame
    // (loopback `nail serve` reports Trusted Types only). The sandbox alone
    // keeps every script, handler, link and refresh inert, although the
    // document has the app's origin.
    await frame.evaluate((el: HTMLIFrameElement, html: string) => {
      el.setAttribute("srcdoc", html);
    }, HOSTILE);
    await expect(frame.contentFrame().locator("script")).toHaveCount(1);
    await expectInert(page, frame);
    expect(await frame.evaluate((el: HTMLIFrameElement) => el.contentDocument?.URL)).toBe(
      "about:srcdoc",
    );
  });
});

test.describe("dark themes", () => {
  useServer();

  test("designed mail on paper in Graphite and Nocturne; frames measure again after a theme switch", async ({
    page,
  }) => {
    await deliverConversation();
    await openInDarkTheme(page, server, "graphite");
    const { long, short } = await openConversation(page);
    // The newsletter (a layout table) keeps a light paper in the dark theme.
    const paper = await frameIn(long)
      .contentFrame()
      .locator("body")
      .evaluate((b) => getComputedStyle(b).backgroundColor);
    expect(paper).toBe("rgb(255, 255, 255)");
    expect((await expectFitted(frameIn(short))).frame).toBeLessThan(120);
    expect((await short.boundingBox())?.height ?? 0).toBeLessThan(200);
    await expectFitted(frameIn(long));
    await page.screenshot({ path: screenshotPath("graphite-local-1280x1000") });

    // A theme switch rebuilds the documents; they are measured again.
    const graphite = await frameIn(short).getAttribute("srcdoc");
    await rpcOk(server, { type: "SetTheme", theme_id: "nocturne" }, "Themes");
    await expect
      .poll(() => frameIn(short).getAttribute("srcdoc"), { timeout: 15_000 })
      .not.toBe(graphite);
    await expectFitted(frameIn(short));
    await expectFitted(frameIn(long));
    await short.scrollIntoViewIfNeeded();
    await page.screenshot({ path: screenshotPath("nocturne-local-1280x1000") });
  });
});

test.describe("the remote UI's sandbox origin", () => {
  useServer();

  test("frames report their height through the hash-pinned script; mail scripts stay inert; Trusted Types enforced", async ({
    page,
  }) => {
    await deliverConversation();
    await control(server, "imap-deliver", {
      raw: mail({
        from: "Mallory <mallory@club.example>",
        subject: "Hostile mail",
        id: "hostile",
        date: "Mon, 05 Oct 2026 11:00:00 +0000",
        html: HOSTILE,
      }),
    });
    const { remote, context, phone } = await pairedRemoteMail(page, server);

    const { long, short } = await openConversation(phone);
    const shortFrame = frameIn(short);
    // Framed from the sandbox origin with the app origin as the parent, scripts in an opaque origin.
    const src = new URL((await shortFrame.getAttribute("src")) ?? "");
    expect(src.port).not.toBe(String(remote.port));
    expect(src.searchParams.get("parent")).toBe(new URL(remote.url).origin);
    await expect(shortFrame).toHaveAttribute("sandbox", "allow-scripts");
    expect(await shortFrame.evaluate((el: HTMLIFrameElement) => el.contentDocument)).toBeNull();

    expect((await expectFitted(shortFrame)).frame).toBeLessThan(120);
    expect((await short.boundingBox())?.height ?? 0).toBeLessThan(200);
    expect((await expectFitted(frameIn(long))).frame).toBeGreaterThan(2000);
    await phone.screenshot({ path: screenshotPath("frost-remote-1280x1000") });

    // Trusted Types: a plain string never reaches a srcdoc sink.
    const refused = await shortFrame.evaluate((el: HTMLIFrameElement) => {
      try {
        el.setAttribute("srcdoc", "<p>untrusted</p>");
        return false;
      } catch (error) {
        return error instanceof TypeError;
      }
    });
    expect(refused).toBe(true);

    await rowOf(phone, "Hostile mail").click();
    const hostile = phone.locator('iframe[data-part="message-view.body"]');
    await expectInert(phone, hostile);
    // The only script in the document is the route's own.
    expect(await hostile.contentFrame().locator("script").count()).toBe(1);
    await context.close();
  });
});

test.describe("the desktop app", () => {
  desktopTests(180_000);
  // Started with its own (longer) timeout and retried once: the start was a
  // known flake on a busy machine (Q2).
  const session = desktopPerFile();

  test("WebView2 measures the local frames: the corpus's newsletter and photo mail fit", async () => {
    const desktop = session.page;
    await showView(desktop, "Mail");
    for (const subject of ["Your weekly garden digest 🌱", "Photo of the new lab bench"]) {
      await rowOf(desktop, subject).click();
      const frame = desktop.locator('iframe[data-part="message-view.body"]').first();
      await expect(frame).toHaveAttribute("sandbox", "allow-same-origin");
      const fit = await expectFitted(frame);
      // Short corpus mail: no 40vh of empty paper below it.
      expect(fit.frame).toBeLessThan(600);
    }
  });
});
