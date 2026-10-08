import { expect, test, type Page } from "@playwright/test";
import { nailPerTest, control } from "../support/nail";
import {
  readerAction,
  messageRows,
  syncAccount,
  DEMO_MULTI_CONVERSATIONS,
  openMail,
  messageList,
  applyLabel,
} from "../support/views";
import { expectSentThrough, newMessage } from "../support/compose";

/**
 * Wave M3b (ADR 0070): a Gmail API account of `nail serve --seed demo-multi`
 * on the mock Gmail server (signed in through the mock OAuth2 server; the
 * account starts switched off so the other suites keep their counts).
 * Controls: `gmail-message`, `gmail-deliver`, `gmail-sent`.
 */

const server = nailPerTest({ seed: "demo-multi" });

const labelsOf = async (subject: string) =>
  (await control<{ id: string; labels: string[] }>(server, "gmail-message", { subject })).labels;

/**
 * Settings → Accounts → Gmail → "Synchronize this account"; waits for its
 * inbox: four conversations (labels → folders; chats are not synced).
 */
async function enableGmail(page: Page) {
  return syncAccount(page, "Gmail", 4, async (settings) => {
    // A Gmail API account shows as one (not as IMAP) with its polling options.
    await expect(settings.getByRole("combobox", { name: "Protocol" })).toHaveValue("gmail");
    await expect(
      settings.getByRole("spinbutton", { name: "Shortest check interval (seconds)" }),
    ).toHaveValue("10");
  });
}

test("a Gmail account syncs, takes a label, archives, sends and picks up server changes", async ({
  page,
}) => {
  await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
  await enableGmail(page);

  // Categories filter the inbox; user labels are listed.
  const tabs = page.locator('[data-part="category-tabs"]');
  await tabs.getByRole("button", { name: /^Social/ }).click();
  await expect(messageRows(page)).toHaveCount(1);
  await expect(messageRows(page).first()).toContainText("Charles mentioned you");
  await tabs.getByRole("button", { name: /^All/ }).click();
  await expect(messageRows(page)).toHaveCount(4);
  const labels = page.locator('[data-part="label-list"]');
  // "Receipts" only shows while it has unread mail; the nested label does.
  await expect(labels.getByRole("button", { name: "Edit label Work/Projects" })).toBeVisible();

  // A label on a message: a tag here, a label change on Gmail.
  await messageRows(page).filter({ hasText: "Lunch on Friday?" }).click();
  await readerAction(page, "Labels…");
  await applyLabel(page, "Receipts");
  await expect.poll(() => labelsOf("Lunch on Friday?"), { timeout: 20_000 }).toContain("Label_2");

  // Archive: the INBOX label goes on Gmail.
  await messageRows(page).filter({ hasText: "Autumn sale" }).click();
  await readerAction(page, "Archive");
  await expect(messageRows(page)).toHaveCount(3);
  await expect
    .poll(async () => (await labelsOf("Autumn sale")).includes("INBOX"), { timeout: 20_000 })
    .toBe(false);

  // Sending goes through the Gmail API (no SMTP server).
  await messageList(page).focus();
  const composer = await newMessage(page, {
    to: "grace@example.org",
    subject: "Sent with the Gmail API",
    text: "Hello Grace",
  });
  await composer.getByRole("button", { name: "Send", exact: true }).click();
  await expect(composer).toBeHidden();
  await expectSentThrough(server, "gmail-sent", "Sent with the Gmail API");

  // A message delivered on the server arrives with the next history poll.
  await control(server, "gmail-deliver", {
    raw: "From: Grace Hopper <grace@example.org>\nTo: ada@gmail.example\nSubject: Compiler news\nMessage-ID: <compiler@example.org>\n\nA-0 works.\n",
  });
  await expect(messageRows(page).filter({ hasText: "Compiler news" })).toHaveCount(1, {
    timeout: 40_000,
  });
});
