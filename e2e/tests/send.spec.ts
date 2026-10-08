import { expect, test, type Page } from "@playwright/test";
import { nailPerTest, control, serverMessages } from "../support/nail";
import {
  composerWindow,
  messageRows,
  DEMO_MULTI_CONVERSATIONS,
  openMail,
  refreshList,
} from "../support/views";
import { defined } from "../support/defined";
import { newMessage, smtpMessages } from "../support/compose";

/**
 * Sending a plain-text mail through the core's outbox to the mock SMTP
 * server of `demo-multi` (undo-send delay: 5 seconds in its accounts.kdl).
 */

const server = nailPerTest({ seed: "demo-multi" });

async function compose(page: Page, subject: string) {
  const text = "Hello Grace,\n\nsee you at the review.\n\nAda";
  const composer = await newMessage(page, { subject, text });
  return { body: composer.getByRole("textbox", { name: "Message" }) };
}

test("a plain-text mail is sent, once cancelled by undo and once for real", async ({ page }) => {
  await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
  const status = page.getByRole("status", { name: "Status" });

  // Send, then undo within the delay: nothing reaches the server and the
  // message is back in the composer.
  const { body } = await compose(page, "Review notes");
  const dialog = composerWindow(page);
  await body.press("Control+Enter");
  await expect(dialog).toBeHidden();
  await expect(status.getByText(/Sending in \d+ seconds?/)).toBeVisible();
  await status.getByRole("button", { name: "Undo" }).click();
  await expect(dialog.getByRole("textbox", { name: "Subject" })).toHaveValue("Review notes");
  expect(await smtpMessages(server)).toEqual([]);

  // Send for real: after the delay the SMTP server has it.
  await dialog.getByRole("button", { name: "Send", exact: true }).click();
  await expect(dialog).toBeHidden();
  await expect(status.getByText("Message sent.")).toBeVisible({ timeout: 15_000 });
  const mails = await smtpMessages(server);
  expect(mails).toHaveLength(1);
  const mail = defined(mails[0], "the sent message");
  expect(mail.from).toBe("ada@work.example");
  expect(mail.to).toEqual(["grace@navy.example"]);
  expect(mail.data).toContain("Subject: Review notes");
  expect(mail.data).toContain("Hello Grace,");

  // The Sent copy is on the IMAP server and in the unified Sent folder.
  await expect
    .poll(async () =>
      (await serverMessages(server, "Sent")).some((m) => m.subject === "Review notes"),
    )
    .toBe(true);
  await page
    .locator('[data-part="folder-tree.section"][data-account-kind="imap"]')
    .locator('[data-part="folder-tree.item"][data-role="sent"]')
    .click();
  await expect(messageRows(page).filter({ hasText: "Review notes" })).toHaveCount(1);
});

test("the Sent copy of a mail sent while IMAP is down is filed later", async ({ page }) => {
  await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
  await control(server, "imap-offline", { offline: true });
  await expect(
    page.locator('[data-part="folder-tree.section"][data-account-kind="imap"]'),
  ).toHaveAttribute("data-status", "offline");

  // SMTP still works: the mail goes out and its copy shows in Sent at once.
  const { body } = await compose(page, "Written offline");
  await body.press("Control+Enter");
  const status = page.getByRole("status", { name: "Status" });
  await expect(status.getByText("Message sent.")).toBeVisible({ timeout: 15_000 });
  await page
    .locator('[data-part="folder-tree.section"][data-account-kind="imap"]')
    .locator('[data-part="folder-tree.item"][data-role="sent"]')
    .click();
  const copy = messageRows(page).filter({ hasText: "Written offline" });
  await expect(copy).toHaveCount(1);
  expect((await serverMessages(server, "Sent")).some((m) => m.subject === "Written offline")).toBe(
    false,
  );

  // Back online, the queued copy reaches the server; the list keeps one row.
  await control(server, "imap-offline", { offline: false });
  await expect
    .poll(
      async () =>
        (await serverMessages(server, "Sent")).filter((m) => m.subject === "Written offline")
          .length,
    )
    .toBe(1);
  await refreshList(page);
  await expect(copy).toHaveCount(1);
});
