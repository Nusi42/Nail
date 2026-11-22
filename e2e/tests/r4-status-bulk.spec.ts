import { expect, test, type Page, type WebSocketRoute } from "@playwright/test";
import { bulkMail, scrollUntilListed } from "../support/bulkMail";
import { nailPerTest } from "../support/nail";
import {
  accountSectionNamed,
  messageRow,
  messageRows,
  moveSelectionTo,
  opsItem,
  sectionInbox,
} from "../support/views";

/**
 * Bulk actions and the status bar (ADR 0230): the "Saving N changes" count
 * follows the core's queue summary, so a bulk move of several hundred
 * messages ends idle with correct lists — also when the event stream lagged
 * and events were lost (simulated by dropping them on the page's WebSocket).
 */

const server = nailPerTest({ seed: "demo-multi" });

/** Messages delivered to the "Work" inbox for the bulk move. */
const BULK = 300;
const mail = bulkMail("R4 bulk", "r4-bulk");
const { subjectOf } = mail;
const NEWEST = subjectOf(BULK);
const OLDEST = subjectOf(1);

test.describe.configure({ timeout: 240_000 });

const folderCounts = (page: Page) =>
  page.locator('[data-part="status-bar.item"][data-item="folder-counts"]');

/** Opens the Work inbox once every bulk message is listed (newest on top). */
const openWorkInboxWithBulk = (page: Page) => mail.openWorkInbox(page, server, BULK);

/**
 * Picks every bulk row: a click on the newest, then (after scrolling until
 * the oldest is loaded and shown) Shift + click on the oldest.
 */
async function pickAllBulk(page: Page): Promise<void> {
  await messageRow(page, NEWEST).click();
  await scrollUntilListed(page, OLDEST);
  await messageRow(page, OLDEST).click({ modifiers: ["Shift"] });
  await expect(messageRow(page, OLDEST)).toHaveAttribute("aria-selected", "true");
}

/** The bulk messages in a server mailbox. */
const bulkOnServer = (mailbox: string) => mail.onServer(server, mailbox);

/** The status bar is idle and both lists show the server's state. */
async function expectSettled(page: Page): Promise<void> {
  await expect.poll(() => bulkOnServer("Archive"), { timeout: 120_000 }).toBe(BULK);
  expect(await bulkOnServer("INBOX")).toBe(0);
  // "Saving N changes" ends: the core's queue is empty.
  await expect(opsItem(page)).toHaveCount(0, { timeout: 30_000 });
  // The inbox lists none of the moved messages, the Archive all of them,
  // and no moved row looks selected anywhere.
  await expect(messageRows(page).filter({ hasText: "R4 bulk " })).toHaveCount(0);
  await expect(messageRows(page).first()).toBeVisible();
  await accountSectionNamed(page, "Work")
    .locator('[data-part="folder-tree.item"][data-role="archive"]')
    .click();
  await expect(messageRow(page, NEWEST)).toHaveCount(1);
  // The Archive's own messages plus every moved one.
  await expect
    .poll(async () => Number.parseInt((await folderCounts(page).textContent()) ?? "", 10))
    .toBeGreaterThanOrEqual(BULK);
  await expect(messageRows(page).and(page.locator('[aria-selected="true"]'))).toHaveCount(0);
  await sectionInbox(accountSectionNamed(page, "Work")).click();
  await expect(messageRows(page).filter({ hasText: "R4 bulk " })).toHaveCount(0);
  await expect(opsItem(page)).toHaveCount(0);
}

test("a bulk move of several hundred messages ends idle with correct lists", async ({ page }) => {
  await openWorkInboxWithBulk(page);
  await pickAllBulk(page);
  await moveSelectionTo(page, "Archive");
  // The moved rows leave the list at once.
  await expect(messageRows(page).filter({ hasText: "R4 bulk " })).toHaveCount(0);
  await expectSettled(page);
});

/** One frame the core sent on the page's WebSocket. */
interface Frame {
  kind?: string;
  event?: { type?: string };
}

/**
 * Forwards the page's WebSocket, dropping every event while `lagging` — as
 * a client that falls behind loses them — and can push frames itself.
 */
async function laggingSocket(page: Page) {
  const state = { lagging: false, dropped: 0, socket: null as WebSocketRoute | null };
  await page.routeWebSocket(/\/api\/ws$/, (ws) => {
    const upstream = ws.connectToServer();
    state.socket = ws;
    ws.onMessage((message) => {
      upstream.send(message);
    });
    upstream.onMessage((message) => {
      if (state.lagging && typeof message === "string") {
        const frame = JSON.parse(message) as Frame;
        if (frame.kind === "event") {
          state.dropped++;
          return;
        }
      }
      ws.send(message);
    });
  });
  return state;
}

test("a lagging event stream still ends idle with correct lists", async ({ page }) => {
  const socket = await laggingSocket(page);
  await openWorkInboxWithBulk(page);
  await pickAllBulk(page);

  // From now on every event of the core is lost: no queue summary, no
  // per-operation status, no list changes.
  socket.lagging = true;
  await moveSelectionTo(page, "Archive");
  await expect(messageRows(page).filter({ hasText: "R4 bulk " })).toHaveCount(0);
  await expect.poll(() => bulkOnServer("Archive"), { timeout: 120_000 }).toBe(BULK);
  expect(socket.dropped).toBeGreaterThan(0);

  // Without a single event the status bar still returns to idle: while
  // writes wait it asks the core for the queue itself.
  await expect(opsItem(page)).toHaveCount(0, { timeout: 30_000 });

  // The lag ends with `Resync` (the core sends it after a lag): the lists
  // are fetched again and nothing stale stays.
  socket.lagging = false;
  const route = socket.socket;
  if (route === null) throw new Error("the page opened no WebSocket");
  route.send(JSON.stringify({ kind: "event", event: { type: "Resync" } }));
  await expectSettled(page);
});
