import { expect, test, type Page } from "@playwright/test";
import { control, startNail, type NailServer } from "../support/nail";
import {
  accountSectionNamed,
  accountSections,
  openSettingsTab,
  runFromPalette,
  sectionInbox,
  showView,
  viewRegion,
} from "../support/views";

/**
 * Accounts against the real core and the `demo-multi` mock servers: an IMAP
 * account ("Work"), a POP3 account ("Home"), SMTP, and a third address
 * (bob@new.example) whose settings the autoconfig mock serves.
 */

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo-multi" });
});

test.afterEach(async () => {
  await server.stop();
});

const rows = (page: Page) => page.locator('[data-part="message-list.row"]');

test("the unified inbox shows both accounts and their status", async ({ page }) => {
  await page.goto(server.url);
  // M5b: one folder-pane section per account, with its kind and state.
  const accounts = accountSections(page);
  await expect(accounts).toHaveCount(5);
  await expect(accounts.nth(0)).toHaveAttribute("data-account-kind", "imap");
  await expect(accounts.nth(1)).toHaveAttribute("data-account-kind", "pop3");
  await expect(accounts.nth(0)).toHaveAttribute("data-status", "online");
  await expect(accounts.nth(1)).toHaveAttribute("data-status", "online");
  // M3b: the demo Gmail account starts switched off (the section says so).
  await expect(accounts.nth(2)).toHaveAttribute("data-account-kind", "gmail");
  await expect(accounts.nth(2)).toHaveAttribute("data-status", "disabled");
  await expect(accounts.nth(2).locator('[data-part="folder-tree.section.status"]')).toHaveText(
    "Disabled",
  );
  // M3c: so do the demo Exchange accounts (Graph, EWS).
  for (const index of [3, 4]) {
    await expect(accounts.nth(index)).toHaveAttribute("data-account-kind", "exchange");
    await expect(accounts.nth(index)).toHaveAttribute("data-status", "disabled");
  }

  // "All inboxes" (the "All accounts" inbox) is selected: it holds both
  // accounts' mail; the mailing-list message both received is shown once.
  await expect(
    page.locator('[data-part="folder-tree.favorite"][data-favorite="all-inboxes"]'),
  ).toHaveAttribute("aria-selected", "true");
  // One row per conversation (P2): work's 30 messages are 15 conversations,
  // home has 3; the mailing-list post in both accounts is one row (listed
  // under the home copy).
  await expect(rows(page)).toHaveCount(15 + 3 - 1);
  await expect(rows(page).filter({ hasText: "Release notes for M1" })).toHaveCount(1);
  await expect(rows(page).and(page.locator('[data-account="work"]'))).toHaveCount(14);
  await expect(rows(page).and(page.locator('[data-account="home"]'))).toHaveCount(3);

  // Each account's own folders are in its section.
  await sectionInbox(accounts.nth(1)).click();
  await expect(rows(page)).toHaveCount(3);
  await expect(rows(page).filter({ hasText: "Garden party on Saturday" })).toHaveCount(1);
});

test("an account is added through discovery against the mock servers", async ({ page }) => {
  await page.goto(server.url);
  await expect(accountSections(page)).toHaveCount(5);
  await runFromPalette(page, "add account", "Add account");
  const dialog = page.getByRole("dialog", { name: "Add account" });
  await dialog.getByRole("textbox", { name: "Your name" }).fill("Bob");
  await dialog.getByRole("textbox", { name: "Email address" }).fill("bob@new.example");
  await dialog.getByRole("button", { name: "Continue" }).click();

  // Autoconfig found the mock servers.
  await expect(dialog.getByText("New Example Mail")).toBeVisible();
  await expect(
    dialog.getByText(/Incoming 127\.0\.0\.1:\d+, outgoing 127\.0\.0\.1:\d+/),
  ).toBeVisible();
  await dialog.getByLabel("Password").fill("new-secret");
  await dialog.getByRole("button", { name: "Test connection" }).click();
  await expect(dialog.getByText("Nail can connect to your servers.")).toBeVisible();
  await expect(dialog.locator('[data-part="account-setup.result"]')).toHaveCount(2);
  await dialog.getByRole("button", { name: "Add account" }).click();
  await expect(dialog.getByText(/“Bob” was added/)).toBeVisible();
  await dialog.getByRole("button", { name: "Go to the inbox" }).click();
  await expect(dialog).toBeHidden();

  const bob = accountSectionNamed(page, "Bob");
  await expect(bob).toHaveAttribute("data-account-kind", "imap");
  await expect(bob).toHaveAttribute("data-status", "online");
  await expect(rows(page).filter({ hasText: "Welcome to New Example Mail" })).toHaveCount(1);
});

test("settings that could not be verified come with a warning", async ({ page }) => {
  await page.goto(server.url);
  await runFromPalette(page, "add account", "Add account");
  const dialog = page.getByRole("dialog", { name: "Add account" });
  // Only DNS records answer for this domain, and they point elsewhere.
  await dialog.getByRole("textbox", { name: "Email address" }).fill("bob@elsewhere.example");
  await dialog.getByRole("button", { name: "Continue" }).click();
  await expect(dialog.getByText("DNS records of the domain")).toBeVisible();
  await expect(
    dialog.locator('[data-part="account-setup.candidate.warning"][data-warning="foreign-hosts"]'),
  ).toHaveText("The servers are not part of your address's domain.");
  await expect(dialog.getByRole("alert")).toContainText("could not be verified");
  // The servers are shown for checking, not just summarized.
  const incoming = dialog.getByRole("group", { name: "Incoming mail" });
  await expect(incoming.getByRole("textbox", { name: "Server" })).toHaveValue("imap.mailhost.test");
});

test("a POP3 account receives mail", async ({ page }) => {
  await page.goto(server.url);
  await sectionInbox(accountSectionNamed(page, "Home")).click();
  await expect(rows(page)).toHaveCount(3);
  await control(server, "pop3-deliver", {
    raw: "From: Grace Hopper <grace@home.example>\nTo: ada@home.example\nSubject: Fresh from the POP3 server\nMessage-ID: <fresh@home.example>\n\nHello!",
  });
  // Check for new mail now (F5 = sync.now).
  await page.locator('[data-part="message-list"]').focus();
  await page.keyboard.press("F5");
  await expect(rows(page)).toHaveCount(4);
  await rows(page).filter({ hasText: "Fresh from the POP3 server" }).click();
  await expect(page.locator('[data-part="message-view.header.subject"]')).toContainText(
    "Fresh from the POP3 server",
  );
});

test("a virtual account merges folders and dedupes a mailing-list message", async ({ page }) => {
  await page.goto(server.url);
  await showView(page, "Settings");
  const settings = viewRegion(page, "Settings");
  await openSettingsTab(page, "Virtual accounts");
  // The Accounts section holds the account settings, too: the editor's card.
  const editor = settings.locator('[data-part="virtual-editor"]');
  await editor.getByRole("button", { name: "New virtual account" }).click();
  await editor.getByRole("textbox", { name: "Name" }).fill("Everything");
  await expect(editor.getByRole("checkbox", { name: "Work" })).toBeChecked();
  await expect(editor.getByRole("checkbox", { name: "Home" })).toBeChecked();
  await expect(editor.getByRole("checkbox", { name: /Merge Inbox, Sent/ })).toBeChecked();
  await editor.getByRole("button", { name: "Create" }).click();
  await expect(editor.getByText("Virtual account saved.")).toBeVisible();
  await showView(page, "Mail");

  const everything = accountSectionNamed(page, "Everything");
  await expect(everything).toHaveAttribute("data-account-kind", "virtual");
  const folders = everything.locator('[data-part="folder-tree.item"]');
  await expect(folders.filter({ hasText: "Inbox" }).first()).toHaveAttribute("data-role", "inbox");
  await folders.and(page.locator('[data-role="inbox"]')).first().click();
  // Both inboxes merged; the list message both accounts got is one row.
  await expect(rows(page)).toHaveCount(17);
  await expect(rows(page).filter({ hasText: "Release notes for M1" })).toHaveCount(1);
  await expect(rows(page).filter({ hasText: "Garden party on Saturday" })).toHaveCount(1);
  await expect(rows(page).filter({ hasText: "Welcome to Nail" })).toHaveCount(1);
});
