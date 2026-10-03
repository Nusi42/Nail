import { expect, test, type Page } from "@playwright/test";
import { control, serverMessages, startNail, type NailServer } from "../support/nail";
import {} from "../support/views";

/**
 * Sending a plain-text mail through the core's outbox to the mock SMTP
 * server of `demo-multi` (undo-send delay: 5 seconds in its accounts.kdl).
 */

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo-multi" });
});

test.afterEach(async () => {
  await server.stop();
});

interface Received {
  from: string;
  to: string[];
  data: string;
}

const received = () => control<Received[]>(server, "smtp-messages");

async function compose(page: Page, subject: string) {
  await page.keyboard.press("c");
  const dialog = page.getByRole("dialog", { name: "New message" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("textbox", { name: "To" }).fill("grace@navy.example");
  await dialog.getByRole("textbox", { name: "Subject" }).fill(subject);
  const body = dialog.getByRole("textbox", { name: "Message" });
  await body.fill("Hello Grace,\n\nsee you at the review.\n\nAda");
  return { body };
}

test("a plain-text mail is sent, once cancelled by undo and once for real", async ({ page }) => {
  await page.goto(server.url);
  await expect(page.locator('[data-part="message-list.row"]')).toHaveCount(17);
  const status = page.getByRole("status", { name: "Status" });

  // Send, then undo within the delay: nothing reaches the server and the
  // message is back in the composer.
  const { body } = await compose(page, "Review notes");
  const dialog = page.getByRole("dialog", { name: "New message" });
  await body.press("Control+Enter");
  await expect(dialog).toBeHidden();
  await expect(status.getByText(/Sending in \d+ seconds?/)).toBeVisible();
  await status.getByRole("button", { name: "Undo" }).click();
  await expect(dialog.getByRole("textbox", { name: "Subject" })).toHaveValue("Review notes");
  expect(await received()).toEqual([]);

  // Send for real: after the delay the SMTP server has it.
  await dialog.getByRole("button", { name: "Send", exact: true }).click();
  await expect(dialog).toBeHidden();
  await expect(status.getByText("Message sent.")).toBeVisible({ timeout: 15_000 });
  const mails = await received();
  expect(mails).toHaveLength(1);
  expect(mails[0]!.from).toBe("ada@work.example");
  expect(mails[0]!.to).toEqual(["grace@navy.example"]);
  expect(mails[0]!.data).toContain("Subject: Review notes");
  expect(mails[0]!.data).toContain("Hello Grace,");

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
  await expect(
    page.locator('[data-part="message-list.row"]').filter({ hasText: "Review notes" }),
  ).toHaveCount(1);
});

test("the Sent copy of a mail sent while IMAP is down is filed later", async ({ page }) => {
  await page.goto(server.url);
  await expect(page.locator('[data-part="message-list.row"]')).toHaveCount(17);
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
  const copy = page
    .locator('[data-part="message-list.row"]')
    .filter({ hasText: "Written offline" });
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
  await page.locator('[data-part="message-list"]').focus();
  await page.keyboard.press("F5");
  await expect(copy).toHaveCount(1);
});
