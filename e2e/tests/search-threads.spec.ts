import { expect, test, type Page } from "@playwright/test";
import { startNail, type NailServer } from "../support/nail";

/** Search with the query language and the thread view, on the real core. */

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo-multi" });
});

test.afterEach(async () => {
  await server.stop();
});

const rows = (page: Page) => page.locator('[data-part="message-list.row"]');

async function ready(page: Page) {
  await page.goto(server.url);
  await expect(rows(page)).toHaveCount(17);
}

test("search with the query language", async ({ page }) => {
  await ready(page);
  const list = page.locator('[data-part="message-list"]');
  await page.keyboard.press("/");
  const input = page.getByRole("searchbox", { name: "Search mail" });
  await expect(input).toBeFocused();

  // Fields, quotes and OR across both accounts of "All accounts".
  await input.fill('subject:"garden party" OR from:users@lists.nail.example');
  await input.press("Enter");
  await expect(list).toHaveAttribute("data-state", "search");
  await expect(rows(page)).toHaveCount(3);
  await expect(rows(page).filter({ hasText: "Garden party on Saturday" })).toHaveCount(1);
  await expect(rows(page).filter({ hasText: "Release notes for M1" })).toHaveCount(2);

  // Flags and negation.
  await input.fill("is:unread NOT from:grace account:home");
  await input.press("Enter");
  await expect(rows(page)).toHaveCount(1);
  await expect(rows(page)).toContainText("Release notes for M1");

  // A mistake is pointed out where it is, before searching.
  await input.fill("frm:ada");
  const error = page.locator('[data-part="search-bar.error"]');
  await expect(error).toBeVisible();
  await expect(error).toHaveAttribute("data-kind", "unknown-field");
  await expect(error.locator('[data-part="search-bar.error.mark"]')).toHaveText("frm:");

  // Escape clears the search: back to the folder.
  await input.press("Escape");
  await expect(list).not.toHaveAttribute("data-state", "search");
  await expect(rows(page)).toHaveCount(17);
});

test("the reader shows a conversation across folders as cards", async ({ page }) => {
  await ready(page);
  const latest = rows(page).filter({ hasText: "Project Nightingale kickoff" }).first();
  await latest.click();
  const thread = page.locator('[data-part="thread-view"]');
  await expect(thread).toBeVisible();
  await expect(thread.locator('[data-part="thread-view.subject"]')).toContainText(
    "Project Nightingale kickoff",
  );
  // Four messages in the inbox, three replies in Sent: seven cards, oldest
  // first (P2 card feed, ADR 0151), the selected one current.
  const feed = page.getByRole("feed");
  const cards = feed.getByRole("article");
  await expect(cards.first()).toHaveAttribute("aria-setsize", "7");
  await expect(cards.and(page.locator('[aria-current="true"]'))).toHaveCount(1);
  await page.getByRole("button", { name: "Expand all" }).click();
  await expect(cards).toHaveCount(7);
  await expect(thread.locator('[data-part="thread-view.fold"]')).toHaveCount(0);

  // Choosing another card makes its message the reader's message.
  const first = cards.first();
  await expect(first).toHaveAttribute("data-state", /\bexpanded\b/);
  await first.locator('[data-part="thread-view.card.toggle"]').focus();
  await expect(first).toHaveAttribute("aria-current", "true");
  await expect(first.locator('[data-part="message-view"]')).toBeVisible();
});
