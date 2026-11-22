import { expect, test, type Page } from "@playwright/test";
import { control, nailPerTest, serverMessages } from "../support/nail";
import {
  accountSectionNamed,
  messageRow,
  messageRows,
  moveSelectionTo,
  opsItem,
  sectionInbox,
} from "../support/views";

/**
 * Failed writes in the status bar (ADR 0230): when the server refuses
 * queued moves, the status bar names how many changes could not be saved
 * and why, with Retry and Dismiss. Retry replays them once the server lets
 * them through; Dismiss gives them up and puts the messages back where the
 * server still has them.
 */

const server = nailPerTest({ seed: "demo-multi" });

test.describe.configure({ timeout: 120_000 });

/**
 * Delivers two read messages `<prefix> 1` and `<prefix> 2` to the Work
 * inbox (read: opening them queues no change of their own).
 */
async function deliverTwo(prefix: string): Promise<[string, string]> {
  const subjects: [string, string] = [`${prefix} 1`, `${prefix} 2`];
  for (const [n, subject] of subjects.entries()) {
    const id = `${prefix.replaceAll(" ", "-")}-${String(n)}@example.net`;
    await control(server, "imap-deliver", {
      flags: ["\\Seen"],
      raw: `From: Grace <grace@example.net>\r\nTo: ada@work.example\r\nSubject: ${subject}\r\nMessage-ID: <${id}>\r\n\r\nBody ${String(n)}\r\n`,
    });
  }
  return subjects;
}

/** The Work inbox's mailbox on the server lets nothing (`true`) or everything open. */
async function refuseInbox(refuse: boolean): Promise<void> {
  await control(server, "imap-deny", { account: "work", mailboxes: refuse ? ["INBOX"] : [] });
}

/** The server's messages in `mailbox` with one of `subjects`. */
async function onServer(mailbox: string, subjects: readonly string[]): Promise<number> {
  const messages = await serverMessages(server, mailbox);
  return messages.filter((m) => subjects.includes(m.subject)).length;
}

/**
 * Opens the Work inbox with two new messages, makes the server refuse the
 * inbox and moves both to the Archive: the status bar reports the failure.
 */
async function failedMoveOfTwo(page: Page, prefix: string): Promise<[string, string]> {
  await page.goto(server.url);
  const inbox = sectionInbox(accountSectionNamed(page, "Work"));
  await inbox.click();
  const subjects = await deliverTwo(prefix);
  await expect(messageRow(page, subjects[1])).toHaveCount(1, { timeout: 60_000 });
  await messageRow(page, subjects[0]).click();
  await messageRow(page, subjects[1]).click({ modifiers: ["ControlOrMeta"] });
  await expect(opsItem(page)).toHaveCount(0);
  await refuseInbox(true);
  await moveSelectionTo(page, "Archive");
  // The rows leave the inbox at once.
  await expect(messageRows(page).filter({ hasText: prefix })).toHaveCount(0);
  const item = opsItem(page);
  await expect(item).toHaveAttribute("data-state", "error", { timeout: 60_000 });
  // The two moves, and the next message marked read where the selection
  // went: the server refuses the whole folder.
  await expect(item).toContainText(/\d+ changes could not be saved: \S/);
  return subjects;
}

test("Retry saves the failed changes once the server lets them through", async ({ page }) => {
  const subjects = await failedMoveOfTwo(page, "R4 retried");
  await refuseInbox(false);
  await opsItem(page).getByRole("button", { name: "Retry" }).click();
  await expect(opsItem(page)).toHaveCount(0, { timeout: 30_000 });
  await expect.poll(() => onServer("Archive", subjects), { timeout: 30_000 }).toBe(2);
  expect(await onServer("INBOX", subjects)).toBe(0);
  await expect(messageRows(page).filter({ hasText: "R4 retried" })).toHaveCount(0);
});

test("Dismiss gives the failed changes up and shows the messages where they are", async ({
  page,
}) => {
  const subjects = await failedMoveOfTwo(page, "R4 dismissed");
  await opsItem(page).getByRole("button", { name: "Dismiss" }).click();
  await expect(opsItem(page)).toHaveCount(0, { timeout: 30_000 });
  // The server never moved them: they are listed in the inbox again.
  for (const subject of subjects) {
    await expect(messageRow(page, subject)).toHaveCount(1, { timeout: 30_000 });
  }
  expect(await onServer("INBOX", subjects)).toBe(2);
  expect(await onServer("Archive", subjects)).toBe(0);
  await refuseInbox(false);
});
