import { expect, test, type BrowserContext, type Locator, type Page } from "@playwright/test";
import { desktopPerFile, desktopTests, shellControl } from "../support/desktop";
import { control, nailOnDemand } from "../support/nail";
import { pairedRemoteMail } from "../support/remote";
import {
  composerWindow,
  DEMO_MULTI_CONVERSATIONS,
  discardComposer,
  messageRow,
  messageRows,
  openMail,
  refreshList,
  showView,
  windowById,
} from "../support/views";

/**
 * Links in mail (ADR 0206): a click on a link in an HTML mail never
 * navigates the reader's frame. Web links open in a new browser tab (web
 * mode, remote UI; `noopener`, `noreferrer`) or in the system browser (the
 * desktop app, observed through its test control); `mailto:` links open
 * Nail's composer. While a link is hovered, a status overlay at the
 * reader's bottom edge shows its target; a link the core flagged shows its
 * warning there and asks before it opens.
 */

const server = nailOnDemand();

const SUBJECT = "We value your feedback";
const TRACKING = "https://track.example.net/e/abc123/click";
const PICTURE = "https://track.example.net/e/abc123/picture";
const PHISHING = "https://login.example.net/account";

const NEWSLETTER = [
  "<p>How was your order?</p>",
  `<p><a href="${TRACKING}"><span>I am fully satisfied.</span></a></p>`,
  `<p><a href="${PICTURE}"><img alt="Five stars" width="120" height="24"></a></p>`,
  `<p>Your account: <a href="${PHISHING}">https://bank.example.com/account</a></p>`,
  '<p><a href="mailto:grace@example.org?subject=Feedback">Write to us</a></p>',
].join("");

/** Delivers an HTML mail to the IMAP account and opens it in the reader; its frame. */
async function deliverAndOpen(
  page: Page,
  mail: { subject: string; id: string; html: string; firstText: string },
): Promise<Locator> {
  await control(server, "imap-deliver", {
    raw: [
      "From: Shop <news@shop.example.com>",
      "To: ada@work.example",
      `Subject: ${mail.subject}`,
      `Message-ID: <${mail.id}@shop.example.com>`,
      "Date: Mon, 26 Oct 2026 08:00:00 +0000",
      "MIME-Version: 1.0",
      "Content-Type: text/html; charset=utf-8",
      "",
      mail.html,
      "",
    ].join("\r\n"),
  });
  await refreshList(page);
  const row = messageRow(page, mail.subject).first();
  await expect(row).toBeVisible({ timeout: 20_000 });
  await row.click();
  const frame = page.locator('iframe[data-part="message-view.body"]').first();
  await expect(frame.contentFrame().getByText(mail.firstText)).toBeVisible();
  return frame;
}

/** Delivers the newsletter and opens it in the reader. */
const openNewsletter = (page: Page) =>
  deliverAndOpen(page, {
    subject: SUBJECT,
    id: "feedback",
    html: NEWSLETTER,
    firstText: "I am fully satisfied.",
  });

const status = (page: Page) => page.locator('[data-part="message-view.link-status"]');

/** The frame's document is still the mail (it never navigated). */
async function expectNotNavigated(frame: Locator): Promise<void> {
  await expect(frame.contentFrame().getByText("How was your order?")).toBeVisible();
}

/** Serves the example links locally, so a new tab never reaches the network. */
async function serveLinks(context: BrowserContext): Promise<void> {
  await context.route("https://*.example.net/**", (route) =>
    route.fulfill({ status: 200, contentType: "text/plain", body: "landing page" }),
  );
}

/** Clicks `target` and returns the tab it opened (checked: no opener). */
async function clickForTab(context: BrowserContext, target: Locator): Promise<Page> {
  const [tab] = await Promise.all([context.waitForEvent("page"), target.click()]);
  await tab.waitForLoadState();
  expect(await tab.evaluate(() => window.opener as unknown)).toBeNull();
  return tab;
}

/** A fresh `nail serve --seed demo-multi` for each test of the calling describe block. */
function useServer(): void {
  test.beforeEach(async () => {
    await server.start({ seed: "demo-multi" });
  });
}

/** Opens the app in web mode with the newsletter in the reader; its frame and document. */
async function openInWebMode(page: Page, context: BrowserContext) {
  await serveLinks(context);
  await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
  const frame = await openNewsletter(page);
  return { frame, mail: frame.contentFrame() };
}

test.describe("web mode", () => {
  useServer();

  test("links open in a new tab, mailto: in the composer; the frame never navigates", async ({
    page,
    context,
  }) => {
    const { frame, mail } = await openInWebMode(page, context);

    // Hovering shows the target over the reader's bottom edge.
    await expect(status(page)).toHaveCount(0);
    await mail.getByText("I am fully satisfied.").hover();
    await expect(status(page)).toBeVisible();
    await expect(status(page)).toContainText(TRACKING);
    await expect(status(page)).not.toHaveAttribute("data-state", "warning");
    // It covers the content instead of moving it.
    const dock = page.locator('[data-part="message-view.link-dock"]');
    expect((await dock.boundingBox())?.height ?? -1).toBe(0);

    // A link around a span opens in a tab of its own.
    const tab = await clickForTab(context, mail.getByText("I am fully satisfied."));
    expect(tab.url()).toBe(TRACKING);
    await tab.close();
    await expectNotNavigated(frame);

    // A link around an image too.
    await mail.getByAltText("Five stars").hover();
    await expect(status(page)).toContainText(PICTURE);
    const picture = await clickForTab(context, mail.getByAltText("Five stars"));
    expect(picture.url()).toBe(PICTURE);
    await picture.close();
    await expectNotNavigated(frame);

    // Leaving the mail hides the status.
    await page.locator('[data-part="thread-view.subject"]').hover();
    await expect(status(page)).toHaveCount(0);

    // Keyboard: a focused link shows its target, Enter follows it.
    await mail.getByRole("link", { name: "I am fully satisfied." }).focus();
    await expect(status(page)).toContainText(TRACKING);
    const [keyed] = await Promise.all([context.waitForEvent("page"), page.keyboard.press("Enter")]);
    await keyed.waitForLoadState();
    expect(keyed.url()).toBe(TRACKING);
    await keyed.close();
    await expectNotNavigated(frame);

    // mailto: opens Nail's composer, addressed.
    await mail.getByText("Write to us").click();
    const composer = composerWindow(page);
    await expect(composer).toBeVisible();
    await expect(composer).toContainText("grace@example.org");
    await discardComposer(page, composer);
    await expectNotNavigated(frame);
  });

  test("a link that shows one address but goes to another warns and asks first", async ({
    page,
    context,
  }) => {
    const { frame, mail } = await openInWebMode(page, context);
    const phishing = mail.getByText("https://bank.example.com/account");

    await phishing.hover();
    await expect(status(page)).toHaveAttribute("data-state", "warning");
    await expect(status(page)).toContainText(PHISHING);
    await expect(status(page)).toContainText(
      "A link shows bank.example.com but goes to login.example.net.",
    );

    // Cancel: nothing opens.
    let opened = 0;
    context.on("page", () => {
      opened += 1;
    });
    await phishing.click();
    const dialog = windowById(page, "mail-open-link");
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText("This link may not go where it seems to.");
    await expect(dialog).toContainText(`Real target: ${PHISHING}`);
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toHaveCount(0);
    expect(opened).toBe(0);
    await expectNotNavigated(frame);

    // Open anyway: it opens in a new tab.
    await phishing.click();
    await expect(dialog).toBeVisible();
    const tab = await clickForTab(context, dialog.getByRole("button", { name: "Open anyway" }));
    expect(tab.url()).toBe(PHISHING);
    await tab.close();
    await expectNotNavigated(frame);
  });
});

test.describe("the warnings banner", () => {
  useServer();

  test("many tracked links: grouped, a few entries, one-line targets, Show all", async ({
    page,
  }) => {
    await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
    const shown = [
      "www.shop.example.com",
      "help.example.org",
      "news.example.com",
      "shop.example.org",
    ];
    const links = Array.from({ length: 10 }, (_, i) => {
      const host = shown[i % shown.length] ?? "";
      const target = `https://tracker.example.net/c/${String(i)}/${"q".repeat(300)}`;
      return `<p><a href="${target}">https://${host}/offer/${String(i)}</a></p>`;
    }).join("");
    const frame = await deliverAndOpen(page, {
      subject: "Ten offers",
      id: "offers",
      html: `<p>Our offers this week</p>${links}`,
      firstText: "Our offers this week",
    });
    const banner = page.locator('[data-part="security-warnings"]').first();
    await expect(banner).toContainText("Nail found 10 things to be careful about in this message.");
    const items = banner.locator('[data-part="security-warnings.item"]');
    await expect(items).toHaveCount(3);
    await expect(items.first()).toContainText("(3 links)");
    // One line per target: the banner stays short and the mail stays in view.
    const lineHeight = await items
      .first()
      .locator('[data-part="security-warnings.target"]')
      .evaluate((el) => el.getBoundingClientRect().height);
    expect(lineHeight).toBeLessThan(30);
    expect((await banner.boundingBox())?.height ?? 1000).toBeLessThan(320);
    const frameTop = (await frame.boundingBox())?.y ?? 10_000;
    expect(frameTop).toBeLessThan(page.viewportSize()?.height ?? 0);

    const toggle = banner.getByRole("button", { name: "Show all 4 warnings (1 more)" });
    await expect(toggle).toHaveAttribute("aria-expanded", "false");
    await toggle.click();
    await expect(items).toHaveCount(4);
    const fewer = banner.getByRole("button", { name: "Show fewer" });
    await expect(fewer).toHaveAttribute("aria-expanded", "true");
    await fewer.click();
    await expect(items).toHaveCount(3);
  });
});

test.describe("the remote UI's sandbox origin", () => {
  useServer();

  test("the hash-pinned script reports hovers and clicks; links open in the session's browser", async ({
    page,
  }) => {
    await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
    await openNewsletter(page);
    const { context, phone } = await pairedRemoteMail(page, server);
    await serveLinks(context);
    const row = messageRow(phone, SUBJECT).first();
    await expect(row).toBeVisible({ timeout: 30_000 });
    await row.click();
    const frame = phone.locator('iframe[data-part="message-view.body"]').first();
    await expect(frame).toHaveAttribute("sandbox", "allow-scripts");
    const mail = frame.contentFrame();

    await mail.getByText("I am fully satisfied.").hover();
    await expect(status(phone)).toContainText(TRACKING);
    const tab = await clickForTab(context, mail.getByText("I am fully satisfied."));
    expect(tab.url()).toBe(TRACKING);
    await tab.close();
    await expectNotNavigated(frame);

    // The warning is numbered the same way on the sandbox origin.
    const phishing = mail.getByText("https://bank.example.com/account");
    await phishing.hover();
    await expect(status(phone)).toHaveAttribute("data-state", "warning");
    await phishing.click();
    const dialog = windowById(phone, "mail-open-link");
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await expect(dialog).toHaveCount(0);
    await context.close();
  });
});

test.describe("the desktop app", () => {
  desktopTests(180_000);
  const session = desktopPerFile();

  test("a clicked link goes to the system browser, never into the frame", async () => {
    const desktop = session.page;
    await showView(desktop, "Mail");
    const row = messageRows(desktop).filter({ hasText: "Your weekly garden digest" }).first();
    await row.click();
    const frame = desktop.locator('iframe[data-part="message-view.body"]').first();
    await expect(frame).toHaveAttribute("sandbox", "allow-same-origin");
    const link = frame.contentFrame().getByText("bulb guide");
    const target = "https://garden-weekly.example/r/abc123?to=bulbs";

    await link.hover();
    await expect(status(desktop)).toContainText(target);
    await link.click();
    await expect.poll(() => shellControl(desktop, "opened-links")).toEqual([target]);
    await expect(frame.contentFrame().getByText("Autumn is here")).toBeVisible();
  });
});
