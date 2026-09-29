import { expect, test, type Page } from "@playwright/test";
import { control, serverMessages, startNail, type NailServer } from "../support/nail";

/**
 * Writes go through the core's operation queue to the mock IMAP server of
 * the `demo-multi` "Work" account, and server-side changes come back
 * through IDLE.
 */

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo-multi" });
});

test.afterEach(async () => {
  await server.stop();
});

const rows = (page: Page) => page.locator('[data-part="message-list.row"]');

async function openWorkInbox(page: Page) {
  await page.goto(server.url);
  await page.locator('[data-part="account-switcher.item"]').filter({ hasText: "Work" }).click();
  await expect(rows(page)).toHaveCount(15);
}

async function flagsOnServer(mailbox: string, subject: string): Promise<string[] | null> {
  const found = (await serverMessages(server, mailbox)).find((m) => m.subject.includes(subject));
  return found ? found.flags : null;
}

test("flag, move and delete reach the IMAP server", async ({ page }) => {
  await openWorkInbox(page);

  // Flag (s): \Flagged on the server.
  const welcome = rows(page).filter({ hasText: "Welcome to Nail" });
  await welcome.click();
  await page.keyboard.press("s");
  await expect(welcome).toHaveAttribute("data-state", /flagged/);
  await expect.poll(() => flagsOnServer("INBOX", "Welcome to Nail")).toContain("\\Flagged");

  // Archive (e): moved on the server, gone from the list.
  await page.keyboard.press("e");
  await expect(rows(page).filter({ hasText: "Welcome to Nail" })).toHaveCount(0);
  await expect.poll(() => flagsOnServer("Archive", "Welcome to Nail")).toContain("\\Flagged");
  await expect.poll(() => flagsOnServer("INBOX", "Welcome to Nail")).toBeNull();

  // Delete: to the server's Trash.
  const release = rows(page).filter({ hasText: "Release notes for M1" });
  await release.click();
  await page.keyboard.press("Delete");
  await expect(rows(page).filter({ hasText: "Release notes for M1" })).toHaveCount(0);
  await expect.poll(() => flagsOnServer("Trash", "Release notes for M1")).not.toBeNull();
  await expect.poll(() => flagsOnServer("INBOX", "Release notes for M1")).toBeNull();

  // The Trash folder shows it; deleting it there removes it for good.
  await page.locator('[data-part="folder-tree.item"][data-role="trash"]').click();
  const trashed = rows(page).filter({ hasText: "Release notes for M1" });
  await expect(trashed).toHaveCount(1);
  await trashed.click();
  await page.keyboard.press("Delete");
  await expect(rows(page)).toHaveCount(0);
  await expect.poll(() => flagsOnServer("Trash", "Release notes for M1")).toBeNull();
});

test("a flag change on the server appears in the UI (IDLE)", async ({ page }) => {
  await openWorkInbox(page);
  const welcome = rows(page).filter({ hasText: "Welcome to Nail" });
  await expect(welcome).not.toHaveAttribute("data-state", /flagged/);
  await control(server, "imap-flags", {
    subject: "Welcome to Nail",
    flags: ["\\Flagged", "\\Seen"],
  });
  await expect(welcome).toHaveAttribute("data-state", /flagged/);
  await expect(welcome).not.toHaveAttribute("data-state", /unread/);

  // New mail is pushed the same way.
  await control(server, "imap-deliver", {
    raw: "From: Grace Hopper <grace@navy.example>\nSubject: Pushed while idling\nMessage-ID: <pushed@navy.example>\n\nHi",
  });
  await expect(rows(page).filter({ hasText: "Pushed while idling" })).toHaveCount(1);
});

test("writes made offline replay after reconnecting", async ({ page }) => {
  await openWorkInbox(page);
  const work = page.locator('[data-part="account-switcher.item"]').filter({ hasText: "Work" });
  await expect(work).toHaveAttribute("data-status", "online");

  await control(server, "imap-offline", { offline: true });
  await expect(work).toHaveAttribute("data-status", "offline");

  const welcome = rows(page).filter({ hasText: "Welcome to Nail" });
  await welcome.click();
  await page.keyboard.press("s");
  // Applied locally at once and shown as waiting in the status bar.
  await expect(welcome).toHaveAttribute("data-state", /flagged/);
  await expect(page.locator('[data-part="status-bar.item"][data-item="ops"]')).not.toBeEmpty();
  expect(await flagsOnServer("INBOX", "Welcome to Nail")).not.toContain("\\Flagged");

  await control(server, "imap-offline", { offline: false });
  await expect(work).toHaveAttribute("data-status", "online");
  await expect.poll(() => flagsOnServer("INBOX", "Welcome to Nail")).toContain("\\Flagged");
  await expect(welcome).toHaveAttribute("data-state", /flagged/);
});
