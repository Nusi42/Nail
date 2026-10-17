/**
 * The README's animations and stills (ADR 0197), recorded against a fresh
 * `nail serve --seed demo-multi` per scene: made-up people, companies and
 * domains only (the phishing scene's brand "ＮＯＶＡＢＡＮＫ" included).
 * Output: `docs/media/<scene>.gif` / `.png`.
 */
import path from "node:path";
import { expect, test as base, type Page } from "@playwright/test";
import { control, repoRoot, startNail, type NailServer } from "../support/nail";
import { rpc } from "../support/remote";
import { messageRows, readerToolbar, refreshList, showView } from "../support/views";
import { CALENDAR_TIME, CONVERSATION, loadMainWindow, MAIN_TIME } from "../visual/screens.ts";
import { preparePage, seedCalendar } from "../visual/support.ts";
import { HOLD_MS, Recorder } from "./recorder.ts";

const MEDIA = path.join(repoRoot, "docs", "media");

/** A fresh demo server for every scene: scenes change themes, folders and mail. */
const test = base.extend<{ nail: NailServer }>({
  nail: [
    // eslint-disable-next-line no-empty-pattern -- Playwright requires a destructuring pattern for fixtures without dependencies
    async ({}, use) => {
      const server = await startNail({ seed: "demo-multi", locale: "en", demoNow: MAIN_TIME });
      try {
        await use(server);
      } finally {
        await server.stop();
      }
    },
    { timeout: 120_000 },
  ],
});

async function open(page: Page, nail: NailServer): Promise<Recorder> {
  await preparePage(page, MAIN_TIME);
  await loadMainWindow(page, nail.url);
  const recorder = new Recorder(page);
  await recorder.showPointer();
  return recorder;
}

const row = (page: Page, subject: string) => messageRows(page).filter({ hasText: subject }).first();

/** Waits until the compiled theme changed from `before`. */
async function themeChanged(page: Page, before: string | null): Promise<void> {
  await expect.poll(() => themeCss(page)).not.toBe(before);
}
const themeCss = (page: Page) =>
  page.evaluate(() => document.head.querySelector("style[data-nail-theme]")?.textContent ?? null);

function saved(recorder: Recorder, name: string): void {
  const { file, bytes } = recorder.save(MEDIA, name);
  console.log(
    `${path.relative(repoRoot, file)}: ${String(recorder.frames.length)} frames, ${String(Math.round(bytes / 1024))} KiB`,
  );
}

test("hero: the unified inbox with a conversation", async ({ page, nail }) => {
  await preparePage(page, MAIN_TIME);
  await loadMainWindow(page, nail.url);
  await row(page, CONVERSATION).click();
  await expect(page.getByRole("feed")).toBeVisible();
  await page.screenshot({ path: path.join(MEDIA, "hero.png") });
  const before = await themeCss(page);
  await rpc(nail, { type: "SetTheme", theme_id: "nocturne" });
  await themeChanged(page, before);
  await page.screenshot({ path: path.join(MEDIA, "hero-dark.png") });
});

test("conversations: rows, the card feed, older messages", async ({ page, nail }) => {
  const rec = await open(page, nail);
  await rec.shot(1200);
  await rec.click(row(page, CONVERSATION), {
    settle: () => expect(page.getByRole("feed")).toBeVisible(),
    hold: 1600,
  });
  const older = page.getByRole("button", { name: /older messages/ });
  await rec.click(older, { hold: 1400 });
  const card = page
    .locator('[data-part="thread-view.card"]')
    .filter({ hasText: "Grace Hopper" })
    .first();
  await rec.click(card, { hold: 1800 });
  saved(rec, "conversations");
});

test("triage: archive a conversation, undo", async ({ page, nail }) => {
  const rec = await open(page, nail);
  await rec.click(row(page, "Dinner on Saturday?"), { hold: 1000 });
  await rec.click(readerToolbar(page).getByRole("button", { name: "Archive", exact: true }), {
    settle: () => expect(page.locator('[data-part="mail-actions.toast"]')).toBeVisible(),
    hold: 1400,
  });
  await rec.click(
    page.locator('[data-part="mail-actions.toast"]').getByRole("button", { name: "Undo" }),
    {
      settle: () => expect(row(page, "Dinner on Saturday?")).toBeVisible(),
      hold: 1600,
    },
  );
  saved(rec, "triage");
});

test("themes: five built-in themes, switched live", async ({ page, nail }) => {
  const rec = await open(page, nail);
  await rec.click(row(page, CONVERSATION), {
    settle: () => expect(page.getByRole("feed")).toBeVisible(),
    hold: 1300,
  });
  for (const theme of ["nocturne", "sage", "graphite", "high-contrast", "frost"]) {
    const before = await themeCss(page);
    await rpc(nail, { type: "SetTheme", theme_id: theme });
    await themeChanged(page, before);
    await rec.shot(1300);
  }
  saved(rec, "themes");
});

test("palette: every action a few keys away", async ({ page, nail }) => {
  const rec = await open(page, nail);
  await rec.shot(900);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await rec.press("Control+k", { settle: () => expect(palette).toBeVisible(), hold: 700 });
  await rec.click(palette.getByRole("combobox", { name: "Search actions" }), { hold: 200 });
  await rec.type("calendar", 1000);
  await rec.click(palette.getByRole("option", { name: /^Calendar Ctrl\+2/ }), {
    settle: () =>
      expect(page.getByRole("region", { name: "Calendar view", exact: true })).toBeVisible(),
    hold: 1800,
  });
  saved(rec, "palette");
});

test("safety: a disguised sender is flagged, the source is one click away", async ({
  page,
  nail,
}) => {
  const rec = await open(page, nail);
  // A made-up bank, written the way phishing mail writes it.
  const name = Buffer.from("Kundenservice ＮＯＶＡＢＡＮＫ").toString("base64");
  const raw = [
    `From: =?UTF-8?B?${name}?= <noreply@xn--ovabank-2jb.xn--om-6la>`,
    "To: ada@work.example",
    "Subject: Your certificate expires today",
    "Date: Wed, 30 Sep 2026 09:40:00 +0000",
    "Message-ID: <readme-lookalike@example.org>",
    "MIME-Version: 1.0",
    "Content-Type: text/plain; charset=utf-8",
    "",
    "Renew your online banking certificate now, or your account will be locked.",
    "",
  ].join("\r\n");
  await control(nail, "imap-deliver", { raw });
  await refreshList(page);
  const phishing = row(page, "Your certificate expires today");
  await expect(phishing).toBeVisible({ timeout: 20_000 });
  await rec.shot(900);
  await rec.click(phishing, {
    settle: () => expect(page.locator('[data-part="security-warnings"]')).toBeVisible(),
    hold: 2600,
  });
  const card = page.locator('[data-part="thread-view.card"]').last();
  await rec.click(card.getByRole("button", { name: "More actions for this message" }), {
    hold: 700,
  });
  const source = page.locator('[data-part="window"][data-window^="message-source:"]');
  await rec.click(page.getByRole("menu").getByRole("menuitem", { name: "Show source" }), {
    settle: () => expect(source).toBeVisible(),
    hold: 2400,
  });
  saved(rec, "safety");
});

test("folders: a new folder on the server, right from the folder pane", async ({ page, nail }) => {
  const rec = await open(page, nail);
  const archive = page
    .locator('[data-part="folder-tree.item"][data-account="work"]')
    .filter({ has: page.getByText("Archive", { exact: true }) })
    .first();
  await rec.click(archive, { button: "right", hold: 900 });
  await rec.click(page.getByRole("menu").getByRole("menuitem", { name: "New subfolder…" }), {
    hold: 600,
  });
  await rec.type("Receipts 2026", 500);
  const dialog = page.locator('[data-part="window"][data-window="folder-name"]');
  await rec.click(dialog.getByRole("button", { name: "Create" }), {
    settle: () => expect(page.getByText("Receipts 2026", { exact: true })).toBeVisible(),
    hold: 1800,
  });
  saved(rec, "folders");
});

test("compose: a new message", async ({ page, nail }) => {
  const rec = await open(page, nail);
  const composer = page.locator('[data-part="window"][data-window^="composer"]');
  await rec.click(page.getByRole("button", { name: "New message" }).first(), {
    settle: () => expect(composer).toBeVisible(),
    hold: 700,
  });
  await rec.click(composer.getByRole("textbox", { name: "To" }), { hold: 200 });
  await rec.type("grace@example.org", 500);
  await rec.click(composer.getByRole("textbox", { name: "Subject" }), { hold: 200 });
  await rec.type("Build server is back", 400);
  await rec.click(composer.getByRole("textbox", { name: "Message" }), { hold: 200 });
  await rec.type("Hi Grace, thanks for the quick fix! Ada", HOLD_MS * 2);
  saved(rec, "compose");
});

test("calendar: the week", async ({ page, nail }) => {
  await seedCalendar(nail);
  await preparePage(page, CALENDAR_TIME);
  await loadMainWindow(page, nail.url);
  const calendar = await showView(page, "Calendar");
  await expect(calendar.getByRole("button", { name: /^Design review,/ }).first()).toBeVisible();
  await page.screenshot({ path: path.join(MEDIA, "calendar.png") });
});
