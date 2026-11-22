import { expect, test } from "@playwright/test";
import { nailPerTest } from "../support/nail";
import { discardSettings, settingsSaveBar } from "../support/settings";
import { openSettings, showView, toolbarAction, viewRegion, windowById } from "../support/views";
import { expectDestructiveConfirm, openMailAs, unsavedPrompt } from "../support/windowTour";

/**
 * The R6 review's fixes (ADR 0239) against `nail serve --seed demo-multi`:
 *
 * - deleting a contact asks in a destructive confirmation (focus on
 *   Cancel, Esc deletes nothing; "Delete contact" deletes), on Windows and
 *   mirrored on macOS;
 * - a contact group's unsaved edits ask with the shared prompt when the
 *   Contact groups window closes;
 * - Settings → Keyboard shortcuts: a new shortcut waits for the section's
 *   Save (the bar shows; Discard drops it).
 */

const server = nailPerTest({ seed: "demo-multi" });

for (const os of ["windows", "macos"] as const) {
  test(`deleting a contact asks first (${os})`, async ({ page }) => {
    await openMailAs(page, server.url, os);
    await showView(page, "Contacts");
    const contacts = viewRegion(page, "Contacts");
    const grace = contacts.getByRole("option", { name: /Grace Hopper/ });
    await grace.click();
    const card = page.locator('[data-part="contact-card"]');
    await expect(card.getByRole("heading", { name: "Grace Hopper" })).toBeVisible();

    await card.getByRole("button", { name: "Delete…" }).click();
    const question = windowById(page, "pim-delete-contacts");
    await expect(question).toContainText("Delete the contact “Grace Hopper”?");
    await expectDestructiveConfirm(question, os);
    await page.keyboard.press("Escape");
    await expect(question).toBeHidden();
    await expect(grace).toBeVisible();

    await card.getByRole("button", { name: "Delete…" }).click();
    await windowById(page, "pim-delete-contacts")
      .getByRole("button", { name: "Delete contact" })
      .click();
    await expect(grace).toHaveCount(0);
  });
}

test("a contact group's unsaved edits ask when the window closes", async ({ page }) => {
  await openMailAs(page, server.url, "windows");
  await showView(page, "Contacts");
  await toolbarAction(page, viewRegion(page, "Contacts"), "Groups…");
  const groups = windowById(page, "pim-contact-groups");
  await expect(groups).toBeVisible();
  const create = groups.locator('[data-part="pim.groups.new"]');
  await create.getByLabel("Group name").fill("Book club");
  await create.getByRole("button", { name: "Create group" }).click();
  const editor = groups.locator('[data-part="pim.groups.editor"]');
  await expect(editor.getByLabel("Group name")).toHaveValue("Book club");

  await editor.getByLabel("Group name").fill("Readers");
  await page.keyboard.press("Escape");
  const prompt = unsavedPrompt(page);
  await expect(prompt).toContainText("Save changes to “Readers”?");
  await prompt.getByRole("button", { name: "Cancel" }).click();
  await expect(editor.getByLabel("Group name")).toHaveValue("Readers");
  await page.keyboard.press("Escape");
  await unsavedPrompt(page).getByRole("button", { name: "Don't save" }).click();
  await expect(groups).toBeHidden();
});

test("a new shortcut waits for the section's Save", async ({ page }) => {
  await openMailAs(page, server.url, "windows");
  const keys = await openSettings(page, "Keyboard shortcuts");
  await keys.getByRole("combobox", { name: "Action" }).selectOption("mail.toggle-flag");
  const capture = keys.getByRole("textbox", { name: "Keys" });
  await capture.focus();
  await page.keyboard.press("Control+Shift+U");
  // Recorded keys are an unsaved change already.
  await expect(settingsSaveBar(page)).toBeVisible();
  await keys.getByRole("button", { name: "Add shortcut" }).click();
  await expect(keys.getByText("New, not saved")).toBeVisible();
  await discardSettings(page);
  await expect(keys.getByText("New, not saved")).toHaveCount(0);
  // The section's title has the focus once the bar went.
  await expect(keys.page().locator('[data-part="settings-page.title"]')).toBeFocused();
});
