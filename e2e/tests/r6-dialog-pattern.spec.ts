import { existsSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { axeViolations } from "../support/axe";
import { nailPerTest } from "../support/nail";
import { messageRows, openSettings, selectInTrash, windowById } from "../support/views";
import {
  expectDialogFooter,
  type ViewerOs,
  unsavedPrompt,
  openMailAs,
} from "../support/windowTour";

/**
 * One dialog pattern everywhere (R6, ADR 0239), on its two reference
 * dialogs against `nail serve --seed demo-multi`:
 *
 * - "Delete permanently?" (a destructive confirmation): the verb and
 *   Cancel in the viewer's system's order, the focus on Cancel, Enter
 *   never deleting;
 * - New theme (an editor): Create and Cancel in that order, the name field
 *   focused; ×, Esc and Cancel with a typed name ask with the shared
 *   unsaved-changes prompt (Save / Don't save / Cancel, Save focused).
 *
 * Each runs as Windows and as macOS (the order mirrors).
 */

const server = nailPerTest({ seed: "demo-multi" });

const SUBJECT = "Welcome to Nail";

/** The page as a viewer on `os`, with the mail view loaded. */
const openAs = (page: Page, os: ViewerOs) => openMailAs(page, server.url, os);

/** Settings → Appearance → New theme as a viewer on `os`: the dialog. */
async function openNewTheme(page: Page, os: ViewerOs): Promise<Locator> {
  await openAs(page, os);
  const settings = await openSettings(page, "Appearance");
  await settings
    .getByRole("region", { name: "Appearance" })
    .getByRole("button", { name: "New theme" })
    .click();
  return windowById(page, "theme-new");
}

for (const os of ["windows", "macos"] as const) {
  test(`Delete permanently: the destructive verb is never the default (${os})`, async ({
    page,
  }) => {
    await openAs(page, os);
    await selectInTrash(page, SUBJECT);
    await page.keyboard.press("Shift+Delete");
    const dialog = windowById(page, "mail-delete-permanently");
    await expect(dialog).toBeVisible();
    await expectDialogFooter(dialog, { os, roles: ["primary", "cancel"], focused: "cancel" });
    await expect(dialog.locator('button[data-role="primary"]')).toHaveAttribute(
      "data-variant",
      "danger",
    );
    expect(await axeViolations(page, '[data-window="mail-delete-permanently"]')).toEqual([]);
    // Enter on the focused Cancel: nothing is deleted.
    await page.keyboard.press("Enter");
    await expect(dialog).toBeHidden();
    await expect(messageRows(page).filter({ hasText: SUBJECT }).first()).toBeVisible();
  });

  test(`New theme: Create and Cancel by role, unsaved name asks (${os})`, async ({ page }) => {
    const dialog = await openNewTheme(page, os);
    const name = dialog.getByRole("textbox", { name: "Name" });
    await expect(name).toBeFocused();
    await expectDialogFooter(dialog, { os, roles: ["primary", "cancel"] });

    // Cancel with a typed name asks; Cancel there keeps editing.
    await name.fill("Ocean Café");
    await dialog.getByRole("button", { name: "Cancel" }).click();
    const prompt = unsavedPrompt(page);
    await expect(prompt).toBeVisible();
    await expect(prompt).toContainText("Save changes to “Ocean Café”?");
    await expectDialogFooter(prompt, {
      os,
      roles: ["primary", "secondary", "cancel"],
      focused: "primary",
    });
    expect(await axeViolations(page, '[data-window^="unsaved-changes:"]')).toEqual([]);
    await prompt.getByRole("button", { name: "Cancel" }).click();
    await expect(prompt).toBeHidden();
    await expect(name).toHaveValue("Ocean Café");

    // Esc asks too; Don't save closes both without a theme file.
    await name.press("Escape");
    await expect(prompt).toBeVisible();
    await prompt.getByRole("button", { name: "Don't save" }).click();
    await expect(prompt).toBeHidden();
    await expect(dialog).toBeHidden();
    expect(existsSync(path.join(server.configDir, "themes", "ocean-cafe.kdl"))).toBe(false);
  });
}

test("New theme: Save in the prompt creates the theme (×)", async ({ page }) => {
  const dialog = await openNewTheme(page, "windows");
  await dialog.getByRole("textbox", { name: "Name" }).fill("Harbour");
  await dialog.locator('[data-part="window.close"]').click();
  const prompt = unsavedPrompt(page);
  await expect(prompt).toBeVisible();
  // Enter runs Save (focused, not destructive).
  await page.keyboard.press("Enter");
  await expect(dialog).toBeHidden();
  await expect
    .poll(() => existsSync(path.join(server.configDir, "themes", "harbour.kdl")))
    .toBe(true);
});
