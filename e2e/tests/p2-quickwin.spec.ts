import { writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { nailPerTest } from "../support/nail";
import { expectNoIssuesIn } from "../support/responsive";
import { messageRows, DEMO_MULTI_CONVERSATIONS, openMail } from "../support/views";

/**
 * The P2 quick win (Design C, ADR 0148) on the real core, kept for layouts
 * written before the card feed (ADR 0151): a reader stack with `thread-view`
 * (no `mode`) above a `message-view` still shows the strip — the newest
 * three messages of the conversation, the older ones folded into "N earlier
 * messages" — and Nail offers once to turn it into the card feed.
 */

/** A user layout from before P2: the strip above the message. */
const STRIP_LAYOUT = [
  'layout "Mine" extends="default" {',
  '    window "mail" {',
  '        split "horizontal" id="panes" sizes="260px 1fr 1.4fr" {',
  '            folder-tree id="folders" show-unread=#true sections="favorites accounts" \\',
  '                favorites="all-inboxes" collapsible=#true account-colors=#true',
  '            message-list id="list" page-size=100 threads="collapsed" {',
  '                row "{from.name}  {count | if-many}  {date | relative}"',
  '                row "{subject}"',
  '                row "{preview}"',
  "            }",
  '            stack id="reader" sizes="auto auto auto 1fr" {',
  '                toolbar id="reader-toolbar" toolbar="reader"',
  '                thread-view id="thread" expand="none"',
  '                security-message id="message-security"',
  '                message-view id="message" headers="from to cc date"',
  "            }",
  "        }",
  "    }",
  "}",
  "",
].join("\n");

const server = nailPerTest({ seed: "demo-multi" }, (s) => {
  writeFileSync(path.join(s.configDir, "layout.kdl"), STRIP_LAYOUT);
});

const offer = (page: Page) => page.getByRole("region", { name: "Read conversations as cards" });

/** Selects the long Nightingale conversation (four in the Inbox, three replies in Sent). */
async function openNightingale(page: Page) {
  const row = messageRows(page).filter({ hasText: "Project Nightingale kickoff" });
  await expect(row).toHaveCount(1);
  await row.click();
  return row;
}

test("an old layout keeps the strip; the count covers the replies from Sent", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
  await offer(page).getByRole("button", { name: "Not now" }).click();
  await expect(offer(page)).toHaveCount(0);

  const row = await openNightingale(page);
  await expect(row).toHaveAttribute("data-state", /\bthread\b/);
  // Four in the Inbox and three replies from Sent (WP201).
  await expect(row.locator('[data-part="message-list.row.count"]')).toHaveText("7");
  const single = messageRows(page).filter({ hasText: "Welcome to Nail" });
  await expect(single.locator('[data-part="message-list.row.count"]')).toBeHidden();

  // The strip: the newest three, the other four folded; no own scroll bar.
  const strip = page.locator('[data-part="thread-view"]');
  await expect(strip).toBeVisible();
  await expect(page.getByRole("feed")).toHaveCount(0);
  const items = strip.locator('[data-part="thread-view.item"]');
  await expect(items).toHaveCount(3);
  const fold = strip.getByRole("button", { name: "4 earlier messages" });
  await expect(fold).toHaveAttribute("aria-expanded", "false");
  const style = await strip.evaluate((el) => {
    const s = getComputedStyle(el);
    return { overflowY: s.overflowY, maxHeight: s.maxHeight };
  });
  expect(style).toEqual({ overflowY: "visible", maxHeight: "none" });
  await expect(items.first().locator('[data-part="thread-view.item.avatar"]')).toHaveCSS(
    "width",
    "28px",
  );
  await fold.click();
  await expect(items).toHaveCount(7);
  await expect(page.locator('[data-part="message-view"]')).toBeVisible();
  await expectNoIssuesIn(page, ["thread-view", "message-list"]);

  // Dismissed once, the offer stays away.
  await page.reload();
  await expect(messageRows(page)).toHaveCount(DEMO_MULTI_CONVERSATIONS);
  await openNightingale(page);
  await expect(page.locator('[data-part="thread-view.item"]')).toHaveCount(3);
  await expect(offer(page)).toHaveCount(0);
});

test("the offer turns the strip into the card feed", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 1000 });
  await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
  await offer(page).getByRole("button", { name: "Use cards" }).click();
  await expect(offer(page)).toHaveCount(0);
  await openNightingale(page);
  const feed = page.getByRole("feed");
  await expect(feed).toBeVisible();
  await expect(feed.getByRole("article").first()).toHaveAttribute("aria-setsize", "7");
  await expect(page.locator('[data-part="thread-view.item"]')).toHaveCount(0);
  // The user's per-message widgets moved into the cards.
  await expect(
    page.locator('[data-part="thread-view.card"] [data-part="message-view"]').first(),
  ).toBeVisible();
});
