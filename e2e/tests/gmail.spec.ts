import { expect, test, type Page } from "@playwright/test";
import { control, startNail, type NailServer } from "../support/nail";
import {
  accountSectionNamed,
  composerWindow,
  openSettingsTab,
  readerAction,
  sectionInbox,
  showView,
  viewRegion,
} from "../support/views";

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
  // `beforeEach` may have failed before assigning it.
  await (server as NailServer | undefined)?.stop();
});

const rows = (page: Page) => page.locator('[data-part="message-list.row"]');

const labelsOf = async (subject: string) =>
  (await control<{ id: string; labels: string[] }>(server, "gmail-message", { subject })).labels;

/** Settings → Accounts → Gmail → "Synchronize this account". */
async function enableGmail(page: Page) {
  await showView(page, "Settings");
  const settings = viewRegion(page, "Settings");
  await openSettingsTab(page, "Accounts");
  await settings.getByRole("button", { name: /^Gmail/ }).click();
  // A Gmail API account shows as one (not as IMAP) with its polling options.
  await expect(settings.getByRole("combobox", { name: "Protocol" })).toHaveValue("gmail");
  await expect(
    settings.getByRole("spinbutton", { name: "Shortest check interval (seconds)" }),
  ).toHaveValue("10");
  await settings.getByRole("checkbox", { name: "Synchronize this account" }).check();
  await settings.getByRole("button", { name: "Save", exact: true }).click();
  await expect(settings.getByText("Settings saved.")).toBeVisible();
  await showView(page, "Mail");
}

test("a Gmail account syncs, takes a label, archives, sends and picks up server changes", async ({
  page,
}) => {
  await page.goto(server.url);
  await expect(rows(page)).toHaveCount(14);
  await enableGmail(page);
  const gmail = accountSectionNamed(page, "Gmail");
  await expect(gmail).toHaveAttribute("data-status", "online", { timeout: 30_000 });
  await sectionInbox(gmail).click();
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
  await readerAction(page, "Labels…");
  const apply = page.getByRole("dialog", { name: "Labels of this message" });
  await apply.getByRole("checkbox", { name: "Receipts" }).check();
  await apply.getByRole("button", { name: "Apply" }).click();
  await expect(apply).toBeHidden();
  await expect.poll(() => labelsOf("Lunch on Friday?"), { timeout: 20_000 }).toContain("Label_2");

  // Archive: the INBOX label goes on Gmail.
  await rows(page).filter({ hasText: "Autumn sale" }).click();
  await readerAction(page, "Archive");
  await expect(rows(page)).toHaveCount(3);
  await expect
    .poll(async () => (await labelsOf("Autumn sale")).includes("INBOX"), { timeout: 20_000 })
    .toBe(false);

  // Sending goes through the Gmail API (no SMTP server).
  await page.locator('[data-part="message-list"]').focus();
  await page.keyboard.press("c");
  const composer = composerWindow(page);
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
