import { expect, test, type Page } from "@playwright/test";
import { defined } from "../support/defined";
import { READER_SUBJECT, messageRows, messageList } from "../support/views";

/** The seeded demo data (nail-testkit corpus) in the shared server. */

async function openInbox(page: Page) {
  await page.goto("/");
  // M5b: the unified inbox ("All inboxes" under Favorites) is selected.
  await expect(
    page.locator('[data-part="folder-tree.favorite"][data-favorite="all-inboxes"]'),
  ).toHaveAttribute("aria-selected", "true");
  const rows = messageRows(page);
  await expect(rows.first()).toBeVisible();
  return rows;
}

test("default three-pane layout renders from data-part attributes", async ({ page }) => {
  await page.goto("/");
  const app = page.locator('[data-part="app"]');
  await expect(app).toBeVisible();
  await expect(app).toHaveAttribute("data-layout", "default");

  const panes = page.locator(
    '[data-part="split"][data-node-id="panes"] > [data-part="split.pane"]',
  );
  await expect(panes).toHaveCount(3);
  await expect(panes.nth(0).locator('[data-part="folder-tree"]')).toBeVisible();
  await expect(panes.nth(1).locator('[data-part="message-list"]')).toBeVisible();
  await expect(panes.nth(2).locator('[data-part="message-view"]')).toBeVisible();
  await expect(page.locator('[data-part="status-bar"]')).toBeVisible();

  // Panes sit side by side, in layout order.
  const boxes = await Promise.all([0, 1, 2].map((i) => panes.nth(i).boundingBox()));
  expect(boxes.every((b) => b !== null && b.width > 100)).toBe(true);
  const [first, second, third] = boxes.map((b) => defined(b, "a pane's box"));
  expect(first?.x).toBeLessThan(second?.x ?? 0);
  expect(second?.x).toBeLessThan(third?.x ?? 0);

  // Everything is themeable: no inline styles, no class names in the app.
  const styled = await page.locator('[data-part="app"] [style], [data-part="app"] [class]').count();
  expect(styled).toBe(0);
  const theme = page.locator("style[data-nail-theme]");
  await expect(theme).toHaveCount(1);
});

test("seeded folders and messages appear", async ({ page }) => {
  const rows = await openInbox(page);
  const folders = page.locator('[data-part="folder-tree.item"]');
  await expect(folders).toHaveCount(3);
  await expect(folders.filter({ hasText: "Inbox" })).toHaveAttribute("data-role", "inbox");
  await expect(folders.filter({ hasText: "Sent" })).toHaveAttribute("data-role", "sent");
  await expect(folders.filter({ hasText: "Archive" })).toHaveAttribute("data-role", "archive");
  await expect(
    folders.filter({ hasText: "Inbox" }).locator('[data-part="folder-tree.item.unread-count"]'),
  ).toHaveText(/\d+/);

  await expect(rows).toHaveCount(14);
  await expect(rows.first()).toContainText("Reminder: Nightingale review");
  await expect(page.locator('[data-item="folder-counts"]')).toContainText("29");

  // Ten sent messages in five conversations (P2: one row per conversation).
  await folders.filter({ hasText: "Sent" }).click();
  await expect(rows).toHaveCount(5);
  await expect(rows.filter({ hasText: "Dinner" })).toHaveCount(1);
});

test("the reader shows the body in a sandboxed iframe with remote images blocked", async ({
  page,
}) => {
  const rows = await openInbox(page);
  await rows.filter({ hasText: "Your weekly garden digest" }).click();

  const view = page.locator('[data-part="message-view"]');
  await expect(page.locator(READER_SUBJECT)).toContainText("Your weekly garden digest");
  await expect(page.getByRole("region", { name: "Remote content" })).toBeVisible();

  const iframe = view.locator('iframe[data-part="message-view.body"]');
  // P3 (ADR 0164): the app's origin for measuring, never scripts.
  await expect(iframe).toHaveAttribute("sandbox", "allow-same-origin");
  const body = page.frameLocator('iframe[data-part="message-view.body"]');
  await expect(body.locator("h1")).toHaveText("Autumn is here");
  // No remote image survives: sources are gone or local.
  const sources = await body
    .locator("img")
    .evaluateAll((imgs) => imgs.map((img) => img.getAttribute("src") ?? ""));
  expect(sources.filter((src) => /^(https?:)?\/\//i.test(src))).toEqual([]);
  await expect(body.locator("script")).toHaveCount(0);
});

test("hostile HTML is neutralised", async ({ page }) => {
  const rows = await openInbox(page);
  await rows.filter({ hasText: "Action required: verify your mailbox" }).click();
  const frame = page.frameLocator('iframe[data-part="message-view.body"]');
  await expect(frame.locator("body")).not.toBeEmpty();
  await expect(frame.locator("script, iframe, form, object, embed, svg")).toHaveCount(0);
  const handlers = await frame
    .locator("*")
    .evaluateAll(
      (elements) =>
        elements.filter((el) => [...el.attributes].some((a) => a.name.startsWith("on"))).length,
    );
  expect(handlers).toBe(0);
});

test("j/k move through the message list", async ({ page }) => {
  const rows = await openInbox(page);
  const list = messageList(page);
  await list.focus();

  await page.keyboard.press("j");
  await expect(rows.nth(0)).toHaveAttribute("aria-selected", "true");
  const firstSubject = page.locator(READER_SUBJECT);
  await expect(firstSubject).toContainText("Reminder: Nightingale review");

  await page.keyboard.press("j");
  await expect(rows.nth(1)).toHaveAttribute("aria-selected", "true");
  await expect(rows.nth(0)).toHaveAttribute("aria-selected", "false");

  await page.keyboard.press("k");
  await expect(rows.nth(0)).toHaveAttribute("aria-selected", "true");
  await expect(firstSubject).toContainText("Reminder: Nightingale review");
});

test("the German locale falls back to English strings", async ({ page }) => {
  await page.goto("/?lang=de");
  await expect(page.locator("html")).toHaveAttribute("lang", "de");
  await expect(page.getByRole("listbox", { name: "Messages" })).toBeVisible();
  await expect(page.locator('[data-part="message-view.empty"]')).toHaveText(
    "Select a message to read it.",
  );
});
