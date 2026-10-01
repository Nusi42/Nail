import { expect, test, type Page } from "@playwright/test";
import { control, startNail, type NailServer } from "../support/nail";

/**
 * Wave M3b (ADR 0070): a Gmail API account of `nail serve --seed demo-multi`
 * on the mock Gmail server (signed in through the mock OAuth2 server; the
 * account starts switched off so the other suites keep their counts).
 * Controls: `gmail-message`, `gmail-deliver`, `gmail-sent`.
 */

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo-multi" });
});

test.afterEach(async () => {
  await server?.stop();
});

const rows = (page: Page) => page.locator('[data-part="message-list.row"]');
const accountItems = (page: Page) => page.locator('[data-part="account-switcher.item"]');

const labelsOf = async (subject: string) =>
  (await control<{ id: string; labels: string[] }>(server, "gmail-message", { subject })).labels;

/** Settings → Accounts → Gmail → "Synchronize this account". */
async function enableGmail(page: Page) {
  await page
    .getByRole("toolbar", { name: "Main toolbar" })
    .getByRole("button", { name: "Settings" })
    .click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Accounts", exact: true }).click();
  await settings.getByRole("button", { name: /^Gmail/ }).click();
  await settings.getByRole("checkbox", { name: "Synchronize this account" }).check();
  await settings.getByRole("button", { name: "Save", exact: true }).click();
  await expect(settings.getByText("Settings saved.")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(settings).toBeHidden();
}

test("a Gmail account syncs, takes a label, archives, sends and picks up server changes", async ({
  page,
}) => {
  await page.goto(server.url);
  await expect(rows(page)).toHaveCount(17);
  await enableGmail(page);
  const gmail = accountItems(page).filter({ hasText: "Gmail" });
  await expect(gmail).toHaveAttribute("data-status", "online", { timeout: 30_000 });
  await gmail.click();
  // The inbox (labels → folders): four conversations; chats are not synced.
  await expect(rows(page)).toHaveCount(4, { timeout: 30_000 });

  // Categories filter the inbox; user labels are listed.
  const tabs = page.locator('[data-part="category-tabs"]');
  await tabs.getByRole("button", { name: /^Social/ }).click();
  await expect(rows(page)).toHaveCount(1);
  await expect(rows(page).first()).toContainText("Charles mentioned you");
  await tabs.getByRole("button", { name: /^All/ }).click();
  await expect(rows(page)).toHaveCount(4);
  const labels = page.locator('[data-part="label-list"]');
  // "Receipts" only shows while it has unread mail; the nested label does.
  await expect(labels.getByRole("button", { name: "Edit label Work/Projects" })).toBeVisible();

  // A label on a message: a tag here, a label change on Gmail.
  await rows(page).filter({ hasText: "Lunch on Friday?" }).click();
  await page
    .getByRole("toolbar", { name: "Message actions" })
    .getByRole("button", { name: "Labels…" })
    .click();
  const apply = page.getByRole("dialog", { name: "Labels of this message" });
  await apply.getByRole("checkbox", { name: "Receipts" }).check();
  await apply.getByRole("button", { name: "Apply" }).click();
  await expect(apply).toBeHidden();
  await expect.poll(() => labelsOf("Lunch on Friday?"), { timeout: 20_000 }).toContain("Label_2");

  // Archive: the INBOX label goes on Gmail.
  await rows(page).filter({ hasText: "Autumn sale" }).click();
  await page
    .getByRole("toolbar", { name: "Message actions" })
    .getByRole("button", { name: "Archive" })
    .click();
  await expect(rows(page)).toHaveCount(3);
  await expect
    .poll(async () => (await labelsOf("Autumn sale")).includes("INBOX"), { timeout: 20_000 })
    .toBe(false);

  // Sending goes through the Gmail API (no SMTP server).
  await page.locator('[data-part="message-list"]').focus();
  await page.keyboard.press("c");
  const composer = page.getByRole("dialog", { name: "New message" });
  await expect(composer).toBeVisible();
  await composer.getByRole("textbox", { name: "To" }).fill("grace@example.org");
  await composer.getByRole("textbox", { name: "Subject" }).fill("Sent with the Gmail API");
  await composer.getByRole("textbox", { name: "Message" }).fill("Hello Grace");
  await composer.getByRole("button", { name: "Send", exact: true }).click();
  await expect(composer).toBeHidden();
  await expect
    .poll(
      async () =>
        (await control<string[]>(server, "gmail-sent")).some((raw) =>
          raw.includes("Sent with the Gmail API"),
        ),
      { timeout: 30_000 },
    )
    .toBe(true);

  // A message delivered on the server arrives with the next history poll.
  await control(server, "gmail-deliver", {
    raw: "From: Grace Hopper <grace@example.org>\nTo: ada@gmail.example\nSubject: Compiler news\nMessage-ID: <compiler@example.org>\n\nA-0 works.\n",
  });
  await expect(rows(page).filter({ hasText: "Compiler news" })).toHaveCount(1, {
    timeout: 40_000,
  });
});
