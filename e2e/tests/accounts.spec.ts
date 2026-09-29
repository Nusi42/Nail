import { expect, test, type Page } from "@playwright/test";
import { control, startNail, type NailServer } from "../support/nail";

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
const accountItems = (page: Page) => page.locator('[data-part="account-switcher.item"]');

test("the unified inbox shows both accounts and their status", async ({ page }) => {
  await page.goto(server.url);
  const accounts = accountItems(page);
  await expect(accounts).toHaveCount(3);
  await expect(accounts.nth(0)).toHaveAttribute("data-kind", "virtual");
  await expect(accounts.nth(0)).toHaveText(/All accounts/);
  await expect(accounts.nth(1)).toHaveAttribute("data-kind", "imap");
  await expect(accounts.nth(2)).toHaveAttribute("data-kind", "pop3");
  await expect(accounts.nth(1)).toHaveAttribute("data-status", "online");
  await expect(accounts.nth(2)).toHaveAttribute("data-status", "online");

  // "All accounts" is selected: its merged inbox holds both accounts' mail;
  // the mailing-list message both accounts received is shown once.
  await expect(page.locator('[data-part="folder-tree.item"][data-role="inbox"]')).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(rows(page)).toHaveCount(15 + 3 - 1);
  await expect(rows(page).filter({ hasText: "Release notes for M1" })).toHaveCount(1);
  await expect(rows(page).and(page.locator('[data-account="work"]'))).toHaveCount(15);
  await expect(rows(page).and(page.locator('[data-account="home"]'))).toHaveCount(2);

  // Each account's own folders are below it.
  await accounts.nth(2).click();
  await expect(rows(page)).toHaveCount(3);
  await expect(rows(page).filter({ hasText: "Garden party on Saturday" })).toHaveCount(1);
});

test("an account is added through discovery against the mock servers", async ({ page }) => {
  await page.goto(server.url);
  await expect(accountItems(page)).toHaveCount(3);
  await page
    .getByRole("toolbar", { name: "Main toolbar" })
    .getByRole("button", {
      name: "Add account",
    })
    .click();
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

  const bob = accountItems(page).filter({ hasText: "Bob" });
  await expect(bob).toHaveAttribute("data-kind", "imap");
  await expect(bob).toHaveAttribute("data-status", "online");
  await expect(rows(page).filter({ hasText: "Welcome to New Example Mail" })).toHaveCount(1);
});

test("a POP3 account receives mail", async ({ page }) => {
  await page.goto(server.url);
  await accountItems(page).filter({ hasText: "Home" }).click();
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
  await page
    .getByRole("toolbar", { name: "Main toolbar" })
    .getByRole("button", {
      name: "Settings",
    })
    .click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Virtual accounts" }).click();
  await settings.getByRole("button", { name: "New virtual account" }).click();
  await settings.getByRole("textbox", { name: "Name" }).fill("Everything");
  await expect(settings.getByRole("checkbox", { name: "Work" })).toBeChecked();
  await expect(settings.getByRole("checkbox", { name: "Home" })).toBeChecked();
  await expect(settings.getByRole("checkbox", { name: /Merge Inbox, Sent/ })).toBeChecked();
  await settings.getByRole("button", { name: "Create" }).click();
  await expect(settings.getByText("Virtual account saved.")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(settings).toBeHidden();

  const everything = accountItems(page).filter({ hasText: "Everything" });
  await expect(everything).toHaveAttribute("data-kind", "virtual");
  await everything.click();
  const folders = page.locator('[data-part="folder-tree.item"]');
  await expect(folders.filter({ hasText: "INBOX" }).first()).toHaveAttribute("data-role", "inbox");
  await folders.and(page.locator('[data-role="inbox"]')).first().click();
  // Both inboxes merged; the list message both accounts got is one row.
  await expect(rows(page)).toHaveCount(17);
  await expect(rows(page).filter({ hasText: "Release notes for M1" })).toHaveCount(1);
  await expect(rows(page).filter({ hasText: "Garden party on Saturday" })).toHaveCount(1);
  await expect(rows(page).filter({ hasText: "Welcome to Nail" })).toHaveCount(1);
});
