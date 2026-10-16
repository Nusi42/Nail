import { expect, test, type Locator, type Page } from "@playwright/test";
import { control, nailPerTest, screenshotFile, type NailServer } from "../support/nail";
import { showView } from "../support/views";

/**
 * ADR 0196: folders are created, renamed, moved and deleted from the
 * folder pane's right-click menus, on the IMAP server of demo-multi's
 * "work" account; a system folder offers none of it.
 */

const server = nailPerTest({ seed: "demo-multi" });

/** A folder row of the work account by its name. */
function folderRow(page: Page, name: string): Locator {
  return page
    .locator('[data-part="folder-tree.item"][data-account="work"]')
    .filter({ has: page.getByText(name, { exact: true }) })
    .first();
}

async function menuItem(page: Page, row: Locator, item: string): Promise<void> {
  await row.click({ button: "right" });
  await page.getByRole("menu").getByRole("menuitem", { name: item }).click();
}

async function serverMailboxes(nail: NailServer): Promise<string[]> {
  const list = await control<{ name: string }[]>(nail, "imap-mailboxes", { account: "work" });
  return list.map((m) => m.name);
}

test("folders are created, renamed, moved and deleted on the server", async ({ page }) => {
  await page.goto(server.url);
  await showView(page, "Mail");
  const archive = folderRow(page, "Archive");
  await expect(archive).toBeVisible({ timeout: 30_000 });

  // New subfolder of Archive.
  await menuItem(page, archive, "New subfolder…");
  const nameWindow = page.locator('[data-part="window"][data-window="folder-name"]');
  await expect(nameWindow.getByText("New folder in “Archive”")).toBeVisible();
  await nameWindow.getByRole("textbox", { name: "Name" }).fill("Clients");
  await page.screenshot({ path: screenshotFile("folders", "new-subfolder") });
  await nameWindow.getByRole("button", { name: "Create" }).click();
  await expect(nameWindow).toHaveCount(0);
  await expect(folderRow(page, "Clients")).toBeVisible();
  expect(await serverMailboxes(server)).toContain("Archive/Clients");

  // Rename.
  await menuItem(page, folderRow(page, "Clients"), "Rename folder…");
  await nameWindow.getByRole("textbox", { name: "Name" }).fill("Customers");
  await nameWindow.getByRole("button", { name: "Save" }).click();
  await expect(folderRow(page, "Customers")).toBeVisible();
  await expect.poll(() => serverMailboxes(server)).toContain("Archive/Customers");

  // Move to the top of the account.
  await menuItem(page, folderRow(page, "Customers"), "Move folder to…");
  const moveWindow = page.locator('[data-part="window"][data-window="folder-move"]');
  await moveWindow.getByRole("button", { name: "The top of the account" }).click();
  await expect(moveWindow).toHaveCount(0);
  await expect.poll(() => serverMailboxes(server)).toContain("Customers");

  // Delete: into Trash, then for good after asking.
  await menuItem(page, folderRow(page, "Customers"), "Delete folder");
  await expect.poll(() => serverMailboxes(server)).toContain("Trash/Customers");
  await menuItem(page, folderRow(page, "Customers"), "Delete folder");
  const confirm = page.locator('[data-part="window"][data-window="folder-delete"]');
  await expect(confirm.getByText(/the empty folder “Customers” for good/)).toBeVisible();
  await page.screenshot({ path: screenshotFile("folders", "delete-for-good") });
  await confirm.getByRole("button", { name: "Delete for good" }).click();
  await expect(folderRow(page, "Customers")).toHaveCount(0);
  await expect.poll(() => serverMailboxes(server)).not.toContain("Trash/Customers");

  // The Inbox cannot be renamed, moved or deleted.
  await folderRow(page, "Inbox").click({ button: "right" });
  const menu = page.getByRole("menu");
  await expect(menu.getByRole("menuitem", { name: "New subfolder…" })).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: "Rename folder…" })).toHaveCount(0);
  await expect(menu.getByRole("menuitem", { name: "Delete folder" })).toHaveCount(0);
  await page.keyboard.press("Escape");
});
