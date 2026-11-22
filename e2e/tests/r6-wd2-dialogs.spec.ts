import { expect, test, type Page } from "@playwright/test";
import { saveSettings } from "../support/settings";
import { openCalendar } from "../support/calendar";
import { nailPerTest } from "../support/nail";
import { openMail, openSettings, showView, viewRegion, windowById } from "../support/views";
import { emulateViewerOs, expectDialogFooter, type ViewerOs } from "../support/windowTour";

/**
 * R6 (ADR 0239) for calendars, tasks, appearance and customization against
 * `nail serve --seed demo-multi`, as a viewer on Windows and on macOS (the
 * order mirrors):
 *
 * - the event editor: Save and Cancel by role in a pinned footer; Esc with
 *   unsaved changes asks with the shared prompt (Don't save closes);
 * - the task editor: `Delete…` alone at the start; it asks in a destructive
 *   confirmation (focus on Cancel, Enter deletes nothing), "Delete task"
 *   deletes;
 * - Delete theme: a destructive confirmation named "Delete theme";
 * - Settings → Appearance: a picked theme waits for the section's Save.
 */

const server = nailPerTest({ seed: "demo-multi" });

const unsavedPrompt = (page: Page) =>
  page.locator('[data-part="window"][data-window^="unsaved-changes:"]');

async function openAs(page: Page, os: ViewerOs): Promise<void> {
  await emulateViewerOs(page, os);
  await openMail(page, server.url);
}

for (const os of ["windows", "macos"] as const) {
  test(`event editor: one footer, unsaved changes ask (${os})`, async ({ page }) => {
    await openAs(page, os);
    const calendar = await openCalendar(page, { agenda: true });
    await calendar.getByRole("button", { name: "New event" }).click();
    const editor = windowById(page, "pim-event-editor");
    await expect(editor).toBeVisible();
    // A new event has nothing to delete.
    await expectDialogFooter(editor, { os, roles: ["primary", "cancel"] });
    await expect(editor.locator('[data-part="dialog.footer"]')).toBeInViewport();

    await editor.getByLabel("Title", { exact: true }).fill("Quarterly review");
    await page.keyboard.press("Escape");
    const prompt = unsavedPrompt(page);
    await expect(prompt).toContainText("Save changes to “Quarterly review”?");
    await expectDialogFooter(prompt, {
      os,
      roles: ["primary", "secondary", "cancel"],
      focused: "primary",
    });
    await prompt.getByRole("button", { name: "Cancel" }).click();
    await expect(editor).toBeVisible();
    await editor.getByRole("button", { name: "Cancel" }).click();
    await unsavedPrompt(page).getByRole("button", { name: "Don't save" }).click();
    await expect(editor).toBeHidden();
  });

  test(`task editor: Delete… at the start asks first (${os})`, async ({ page }) => {
    await openAs(page, os);
    await showView(page, "Tasks");
    const tasks = viewRegion(page, "Tasks");
    const tree = tasks.getByRole("tree", { name: "Tasks" });
    await tasks.getByLabel(/^Add a task to/).fill("Water the plants");
    await tasks.getByRole("button", { name: "Add", exact: true }).click();
    const task = tree.getByRole("treeitem", { name: /^Water the plants/ });
    await task.focus();
    await page.keyboard.press("Enter");
    const editor = windowById(page, "pim-task-editor");
    await expectDialogFooter(editor, { os, roles: ["primary", "cancel"], start: true });

    await editor.getByRole("button", { name: "Delete…" }).click();
    const question = windowById(page, "pim-delete-task");
    await expect(question).toContainText("Delete the task “Water the plants”?");
    await expectDialogFooter(question, { os, roles: ["primary", "cancel"], focused: "cancel" });
    await expect(question.locator('button[data-role="primary"]')).toHaveAttribute(
      "data-variant",
      "danger",
    );
    // Enter on the focused Cancel deletes nothing.
    await page.keyboard.press("Enter");
    await expect(question).toBeHidden();
    await expect(task).toBeVisible();

    await editor.getByRole("button", { name: "Delete…" }).click();
    await windowById(page, "pim-delete-task").getByRole("button", { name: "Delete task" }).click();
    await expect(editor).toBeHidden();
    await expect(task).toHaveCount(0);
  });
}

test("Delete theme is a destructive confirmation", async ({ page }) => {
  await openAs(page, "windows");
  const settings = await openSettings(page, "Appearance");
  await settings
    .getByRole("region", { name: "Appearance" })
    .getByRole("button", { name: "New theme" })
    .click();
  const create = windowById(page, "theme-new");
  await create.getByRole("textbox", { name: "Name" }).fill("Harbour");
  await create.getByRole("checkbox", { name: "Use the new theme now" }).uncheck();
  await create.getByRole("button", { name: "Create" }).click();
  const themeEditor = windowById(page, "theme-editor");
  await expect(themeEditor.getByLabel("Theme", { exact: true })).toHaveValue("harbour");
  await themeEditor.getByRole("button", { name: "Delete", exact: true }).click();
  const remove = windowById(page, "theme-delete");
  await expect(remove).toContainText("Delete the theme “Harbour”?");
  await expectDialogFooter(remove, {
    os: "windows",
    roles: ["primary", "cancel"],
    focused: "cancel",
  });
  await remove.getByRole("button", { name: "Delete theme" }).click();
  await expect(remove).toBeHidden();
  await expect(themeEditor.getByLabel("Theme", { exact: true })).not.toHaveValue("harbour");
});

test("Settings → Appearance applies a picked theme only on Save", async ({ page }) => {
  await openAs(page, "windows");
  const settings = await openSettings(page, "Appearance");
  const appearance = settings.getByRole("region", { name: "Appearance" });
  const accent = () =>
    page.evaluate(() =>
      getComputedStyle(document.documentElement).getPropertyValue("--nail-color-accent").trim(),
    );
  const before = await accent();
  const nocturne = appearance
    .getByRole("radiogroup", { name: "Themes" })
    .getByRole("radio", { name: /Nocturne/ });
  await nocturne.click();
  await expect(nocturne).toHaveAttribute("aria-checked", "true");
  const bar = page.locator('[data-part="settings-save-bar"]');
  await expect(bar).toContainText("Unsaved changes to “Appearance”");
  await expectDialogFooter(bar, { os: "windows", roles: ["primary", "cancel"] });
  expect(await accent()).toBe(before);
  await saveSettings(page);
  await expect.poll(accent).not.toBe(before);
});
