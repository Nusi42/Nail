import { expect, test } from "@playwright/test";
import { startNail } from "../support/nail";
import { accountSections, sectionInbox } from "../support/views";

test("an IMAP account syncs into the UI and F5 syncs again", async ({ page }) => {
  // The demo corpus served by an in-process mock IMAP server.
  const server = await startNail({ seed: "demo-imap" });
  try {
    const sent: string[] = [];
    const received: string[] = [];
    page.on("websocket", (ws) => {
      ws.on("framesent", (frame) => sent.push(String(frame.payload)));
      ws.on("framereceived", (frame) => received.push(String(frame.payload)));
    });
    await page.goto(server.url);
    // "All inboxes" (the unified view, selected by default), then the account.
    const accounts = accountSections(page);
    await expect(accounts).toHaveCount(1);
    await expect(accounts.nth(0)).toHaveAttribute("data-account-kind", "imap");

    const inbox = sectionInbox(accounts.nth(0));
    await expect(inbox).toBeVisible();
    await inbox.click();
    const rows = page.locator('[data-part="message-list.row"]');
    await expect(rows).toHaveCount(11);

    const status = page.locator('[data-item="sync-progress"]');
    await page.locator('[data-part="message-list"]').focus();
    await page.keyboard.press("F5");
    await expect.poll(() => sent.some((f) => f.includes('"SyncNow"'))).toBe(true);
    await expect.poll(() => received.some((f) => f.includes('"SyncStarted"'))).toBe(true);
    await expect(status).toHaveAttribute("data-state", "idle");
    await expect(status).toHaveText("Up to date");
    await expect(rows).toHaveCount(11);

    await rows.filter({ hasText: "Welcome to Nail" }).click();
    await expect(page.locator('[data-part="message-view.header.subject"]')).toContainText(
      "Welcome to Nail",
    );
  } finally {
    await server.stop();
  }
});
