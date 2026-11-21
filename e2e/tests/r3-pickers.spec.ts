import { expect, test, type Locator, type Page } from "@playwright/test";
import { createMcpClient } from "../support/mcp";
import { nailPerTest } from "../support/nail";
import { expectChips, openChoices, pickOption } from "../support/pickers";
import { expectResponsive } from "../support/responsive";
import { openSettingsTab } from "../support/views";

/**
 * R3 pickers everywhere (ADR 0227) on the real core (`demo-multi`) at
 * 1024×700: the settings fields that name an account, a folder or an
 * address are shared pickers; "Also my addresses" and the other lists are
 * multi-value pickers (chips plus the input); the vacation reply's address
 * pickers offer only the account's own addresses.
 */

const server = nailPerTest({ seed: "demo-multi" });

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 700 });
  await page.goto(server.url);
});

/** The vacation editor of Settings → Vacation reply. */
async function vacationEditor(page: Page): Promise<Locator> {
  const settings = await openSettingsTab(page, "Vacation reply");
  return settings.locator('[data-part="vacation-editor"]');
}

test("the vacation address pickers offer only the account's own addresses", async ({ page }) => {
  const vacation = await vacationEditor(page);
  for (const label of ["Reply from", "Also my addresses"]) {
    await expect(vacation.getByRole("combobox", { name: label })).toHaveValue("");
    const { list } = await openChoices(vacation, label);
    // The Work account's one identity, and none of the most-used correspondents.
    await expect(list.getByRole("option")).toHaveCount(1);
    await expect(list.getByRole("option")).toContainText("ada@work.example");
    await page.keyboard.press("Escape");
    await expect(list).toBeHidden();
  }
  // Typing filters the own addresses; Enter takes the one left.
  const from = vacation.getByRole("combobox", { name: "Reply from" });
  await from.pressSequentially("ada", { delay: 10 });
  await expect(
    vacation.getByRole("listbox", { name: "Choices for Reply from" }).getByRole("option"),
  ).toHaveCount(1);
  await page.keyboard.press("Enter");
  await expect(from).toHaveValue("ada@work.example");
});

test("'Also my addresses' takes several addresses as chips and saves them", async ({ page }) => {
  const vacation = await vacationEditor(page);
  const label = "Also my addresses";
  const field = vacation.getByRole("combobox", { name: label });
  await field.pressSequentially("alias@example.org, other@example.net", { delay: 5 });
  await page.keyboard.press("Enter");
  await expectChips(vacation, label, ["alias@example.org", "other@example.net"]);
  await expect(field).toHaveValue("");
  // A picked suggestion becomes a chip too; the list stays open for the next one.
  const { list } = await openChoices(vacation, label);
  await expect(list).toHaveAttribute("aria-multiselectable", "true");
  await list.getByRole("option").first().click();
  await expect(list).toBeVisible();
  await expect(list.getByRole("option", { selected: true })).toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(
    vacation.getByRole("list", { name: `Chosen for ${label}` }).getByRole("listitem"),
  ).toHaveCount(3);
  // Backspace in the empty field removes the last chip, the chip's button any chip.
  await field.focus();
  await page.keyboard.press("Backspace");
  await vacation.getByRole("button", { name: "Remove other@example.net" }).click();
  await expectChips(vacation, label, ["alias@example.org"]);
  await expectResponsive(page, "r3-picker-chips", {
    sizes: [{ width: 1024, height: 700 }],
    // The multi-value field with its chips.
    scope: '[data-part="vacation-editor"] [data-field="vacation-addresses"]',
    screenshot: true,
  });

  await vacation.getByRole("checkbox", { name: /Send a vacation reply/ }).check();
  await vacation.getByRole("textbox", { name: "Message" }).fill("Away until Monday.");
  await vacation.getByRole("button", { name: "Save", exact: true }).click();
  await expect(vacation.getByText(/^Saved/)).toBeVisible();
  // Read back from the core.
  await page.reload();
  const again = await vacationEditor(page);
  await expectChips(again, label, ["alias@example.org"]);
});

test("the vacation and server-rules account fields are account pickers", async ({ page }) => {
  const vacation = await vacationEditor(page);
  const account = vacation.getByRole("combobox", { name: "Account" });
  await expect(account).toHaveValue("Work");
  // Only accounts with a vacation reply (IMAP and Exchange): not the POP3 account.
  await vacation.getByRole("button", { name: "Show choices for Account" }).click();
  const list = vacation.getByRole("listbox", { name: "Choices for Account" });
  await expect(list.getByRole("option", { name: /^Contoso/ })).toBeVisible();
  await expect(list.getByRole("option", { name: /^Home/ })).toHaveCount(0);
  await list.getByRole("option", { name: /^Contoso/ }).click();
  await expect(account).toHaveValue("Contoso");
  await expect(vacation.getByText(/automatic replies/i).first()).toBeVisible();

  const settings = await openSettingsTab(page, "Server rules");
  const rules = settings.locator('[data-part="sieve-status"]');
  const imap = rules.getByRole("combobox", { name: "Account" });
  await expect(imap).toHaveValue("Work");
  await rules.getByRole("button", { name: "Show choices for Account" }).click();
  await expect(
    rules.getByRole("listbox", { name: "Choices for Account" }).getByRole("option"),
  ).toHaveCount(1);
});

test("the junk settings pick their account with the picker", async ({ page }) => {
  const settings = await openSettingsTab(page, "Junk mail");
  const junk = settings.locator('[data-part="junk-settings"]');
  const account = junk.getByRole("combobox", { name: "Account" });
  await expect(account).toHaveValue("Work");
  // Text that names no account does not switch it; an account's name does.
  await account.fill("nobody");
  await page.keyboard.press("Escape");
  await account.blur();
  await expect(account).toHaveValue("Work");
  await account.fill("home");
  await page.keyboard.press("Escape");
  await account.blur();
  await expect(account).toHaveValue("Home");
  await pickOption(junk, "Account", /^Work/);
  await expect(account).toHaveValue("Work");
  await expect(
    junk.getByRole("checkbox", { name: "Classify arriving mail with Nail's junk filter" }),
  ).toBeVisible();
});

test("searchable decrypted mail is a multi-account picker", async ({ page }) => {
  const settings = await openSettingsTab(page, "Data protection");
  const section = settings.getByRole("region", { name: "Searching encrypted mail" });
  const label = "Accounts with searchable decrypted mail";
  await pickOption(section, label, /^Work/);
  await expectChips(section, label, ["Work"]);
  await expect(settings.getByText(/Decrypted text is indexed/)).toBeVisible();
  await section.getByRole("button", { name: "Remove Work" }).click();
  await expectChips(section, label, []);
  await expect(settings.getByText(/Decrypted text is indexed/)).toBeHidden();
});

test("an MCP grant picks its accounts and hidden folders", async ({ page }) => {
  const { access } = await createMcpClient(page, "Picker agent");
  await access.getByRole("checkbox", { name: "Read mail", exact: true }).check();
  await pickOption(access, "Accounts it may use", /^Work/);
  await expectChips(access, "Accounts it may use", ["Work"]);
  // The hidden folders offer the granted account's folder tree.
  await pickOption(access, "Folders it cannot see", /^Archive/);
  await expectChips(access, "Folders it cannot see", [/Archive/]);
  await access.getByRole("button", { name: "Save access" }).click();
  await expect(access.getByRole("status")).toHaveText("Access saved.");
});

test("the export wizard picks its account and folders", async ({ page }) => {
  const settings = await openSettingsTab(page, "Import and export");
  await settings.getByRole("button", { name: "Export…" }).click();
  const dialog = page.getByRole("dialog", { name: "Export" });
  const account = dialog.getByRole("combobox", { name: "Account" });
  await pickOption(dialog, "Account", /^Work/);
  await expect(account).toHaveValue("Work");
  await pickOption(dialog, "Folders", /^Archive/);
  await expectChips(dialog, "Folders", [/Archive/]);
  await expect(dialog.getByRole("tree", { name: "Choices for Folders" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toBeVisible();
});

test("the key lookup's address field is an address picker", async ({ page }) => {
  const keys = await openSettingsTab(page, "Keys and certificates");
  await keys
    .getByRole("toolbar", { name: "Key tools" })
    .getByRole("button", { name: "Find online" })
    .click();
  const tool = keys.getByRole("region", { name: "Find online" });
  await tool.getByRole("button", { name: "Show choices for Address" }).click();
  await expect(
    tool.getByRole("listbox", { name: "Choices for Address" }).getByRole("option").first(),
  ).toBeVisible();
});
