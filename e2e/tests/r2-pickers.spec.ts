import { expect, test, type Locator, type Page } from "@playwright/test";
import { nailPerTest } from "../support/nail";
import { expectResponsive } from "../support/responsive";
import { openSettingsTab } from "../support/views";
import { pickTheme } from "../support/appearance";

/**
 * R2 shared pickers (ADR 0222) on the real core (`demo-multi`): fields that
 * name a folder or an account are comboboxes with a list to browse, search
 * and pick from (folders as a tree), free text still allowed. The list
 * overlays what is below it, closes with Escape and a click outside, and
 * keeps its looks at 1024×700, in dark mode and in high contrast.
 */

const server = nailPerTest({ seed: "demo-multi" });

/** A new virtual account's first folder source (one merged folder with one source). */
async function sourceRow(page: Page): Promise<Locator> {
  const editor = (await openSettingsTab(page, "Virtual accounts")).locator(
    '[data-part="virtual-editor"]',
  );
  await editor.getByRole("button", { name: "New virtual account" }).click();
  await editor.getByRole("button", { name: "Add merged folder" }).click();
  await editor.getByRole("textbox", { name: "Folder name" }).fill("All inboxes");
  await editor.getByRole("button", { name: "Add folder" }).click();
  return editor.locator('[data-part="virtual-editor.source"]').first();
}

/** The source row shown now (after {@link sourceRow}). */
const sourceRowShown = (page: Page) => page.locator('[data-part="virtual-editor.source"]').first();

const popupOf = (row: Locator) => row.locator('[data-part="picker.popup"]');

/** The popup's background, its highlighted row's and the settings card's around it. */
const looksOf = (popup: Locator) =>
  popup.evaluate((el) => {
    const active = el.querySelector('[data-state~="highlighted"]');
    const card = el.closest('[data-part="settings-page.card"]');
    return {
      card: card ? getComputedStyle(card).backgroundColor : null,
      popup: getComputedStyle(el).backgroundColor,
      active: active ? getComputedStyle(active).backgroundColor : null,
    };
  });

test("a folder picker searches the account's folder tree and picks by keyboard", async ({
  page,
}) => {
  await page.goto(server.url);
  const row = await sourceRow(page);
  const folder = row.getByRole("combobox", { name: "Folder" });
  const remove = row.getByRole("button", { name: "Remove folder" });
  const before = await remove.boundingBox();

  // Browsing shows the tree; it overlays the form instead of pushing it down.
  await row.getByRole("button", { name: "Show choices for Folder" }).click();
  const tree = row.getByRole("tree", { name: "Choices for Folder" });
  await expect(tree.getByRole("treeitem").first()).toBeVisible();
  expect(await remove.boundingBox()).toEqual(before);
  await expect(folder).toBeFocused();

  // Escape closes it; typing searches it; Enter picks the active row.
  await page.keyboard.press("Escape");
  await expect(tree).toBeHidden();
  await folder.pressSequentially("inb", { delay: 10 });
  const inbox = tree.getByRole("treeitem", { name: /inbox/i }).first();
  await expect(inbox).toBeVisible();
  await expect(folder).toHaveAttribute(
    "aria-activedescendant",
    (await inbox.getAttribute("id")) ?? "",
  );
  await page.keyboard.press("Enter");
  await expect(tree).toBeHidden();
  await expect(folder).toHaveValue(/inbox/i);

  // A click outside closes it, too.
  await page.keyboard.press("ArrowDown");
  await expect(tree).toBeVisible();
  await page.mouse.click(5, 5);
  await expect(tree).toBeHidden();

  await page.getByRole("textbox", { name: "Name", exact: true }).fill("Inboxes");
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByText("Virtual account saved.")).toBeVisible();
});

test("an account picker lists the accounts and shows the picked one by name", async ({ page }) => {
  await page.goto(server.url);
  const row = await sourceRow(page);
  const account = row.getByRole("combobox", { name: "Account" });
  await account.click();
  await page.keyboard.press("ArrowDown");
  const list = row.getByRole("listbox", { name: "Choices for Account" });
  await list.getByRole("option", { name: /Home/ }).click();
  await expect(account).toHaveValue("Home");
  // The folder field now browses Home's folders.
  await row.getByRole("button", { name: "Show choices for Folder" }).click();
  await expect(row.getByRole("treeitem").first()).toBeVisible();
});

test("the pickers fit at 1024×700 and keep their looks in a dark theme and high contrast", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1024, height: 700 });
  await page.goto(server.url);
  const row = await sourceRow(page);
  // The source row with its two pickers (the merged folder's own name field
  // beside it is not a picker).
  await expectResponsive(page, "r2-picker-fields", {
    sizes: [{ width: 1024, height: 700 }],
    scope: '[data-part="virtual-editor.source"]',
    screenshot: true,
  });
  await row.getByRole("button", { name: "Show choices for Folder" }).click();
  const popup = popupOf(row);
  await expect(popup).toBeVisible();
  const box = await popup.boundingBox();
  expect(box && box.y + box.height <= 700 + 1 && box.x + box.width <= 1024 + 1).toBe(true);

  const light = await looksOf(popup);
  expect(light.active).not.toBeNull();
  expect(light.popup).toBe(light.card);
  expect(light.active).not.toBe(light.popup);

  // A dark theme and the high-contrast theme: the popup takes the card's
  // background, the highlighted row stands out.
  for (const theme of ["Nocturne", "High contrast"]) {
    const appearance = await openSettingsTab(page, "Appearance");
    await pickTheme(appearance, new RegExp(theme));
    const themed = await sourceRow(page);
    await themed.getByRole("button", { name: "Show choices for Folder" }).click();
    await expect(popupOf(themed)).toBeVisible();
    const looks = await looksOf(popupOf(themed));
    expect(looks.popup, theme).not.toBe(light.popup);
    expect(looks.popup, theme).toBe(looks.card);
    expect(looks.active, theme).not.toBe(looks.popup);
  }
  const highlighted = popupOf(sourceRowShown(page)).locator('[data-state~="highlighted"]');
  await expect(highlighted).toHaveCSS("background-color", "rgb(255, 255, 0)");
});

test("the folder policy settings pick their folder with the picker", async ({ page }) => {
  await page.goto(server.url);
  const settings = await openSettingsTab(page, "Synchronization");
  const section = settings.getByRole("region", { name: "Synchronization per folder" });
  const folder = section.getByRole("combobox", { name: "Folder", exact: true });
  await expect(folder).not.toHaveValue("");
  await folder.fill("");
  await folder.pressSequentially("arch", { delay: 10 });
  await expect(section.getByRole("treeitem", { name: /archive/i }).first()).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(folder).toHaveValue(/archive/i);
});
