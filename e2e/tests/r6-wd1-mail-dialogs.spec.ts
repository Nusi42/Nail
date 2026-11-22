import { expect, test, type Locator, type Page } from "@playwright/test";
import { control, nailPerTest } from "../support/nail";
import {
  composerWindow,
  DEMO_MULTI_CONVERSATIONS,
  folderMenuItem,
  messageList,
  messageRows,
  openMail,
  readerAction,
  showView,
  syncAccount,
  windowById,
  workFolderRow,
} from "../support/views";
import { WELCOME as SUBJECT } from "../support/rules";
import { lengthenAutosave } from "../support/compose";
import {
  emulateViewerOs,
  expectDialogFooter,
  type ViewerOs,
  unsavedPrompt,
} from "../support/windowTour";

/**
 * R6 (ADR 0239) for the mail dialogs against `nail serve --seed demo-multi`:
 * each one's shared footer in the viewer's system's order (Windows and
 * macOS), the button that has the focus, and the close guard of the
 * editors:
 *
 * - the composer: Enter in Subject never sends; × with unsaved changes asks
 *   with the shared prompt (Save draft focused, Don't save, Cancel); Discard
 *   of a saved draft is a destructive question (Discard draft, Keep editing
 *   focused, Enter discards nothing);
 * - Print (Print, Save as PDF…, Cancel), Snooze (Snooze, Cancel) and the
 *   message source (no footer: × closes);
 * - folders: the name window (Create, Cancel; a typed name asks before it
 *   is lost) and "Delete folder" (Cancel focused, Enter deletes nothing);
 * - the move question before Trash (Move focused; Esc back to the picker);
 * - Gmail: "Delete label" (Cancel focused) and "Apply labels" (Save,
 *   Cancel; a changed choice asks); Exchange: the category editor and
 *   "Apply categories".
 */

const server = nailPerTest({ seed: "demo-multi" });

test.describe.configure({ timeout: 120_000 });

/** The page as a viewer on `os` (the composer's autosave lengthened), the mail view loaded. */
async function openAs(page: Page, os: ViewerOs): Promise<void> {
  await emulateViewerOs(page, os);
  await lengthenAutosave(page);
  await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
}

/** The text of the footer button with `role`. */
const footerButton = (dialog: Locator, role: string) =>
  dialog.locator(`[data-part="dialog.footer"] button[data-role="${role}"]`).last();

for (const os of ["windows", "macos"] as const) {
  test(`composer: Enter never sends, × asks, Discard draft is never the default (${os})`, async ({
    page,
  }) => {
    await openAs(page, os);
    await messageList(page).focus();
    await page.keyboard.press("c");
    const composer = composerWindow(page);
    await expect(composer).toBeVisible();
    const subject = composer.getByRole("textbox", { name: "Subject" });
    await subject.fill("Plans for Friday");
    await subject.press("Enter");
    await expect(composer).toBeVisible();
    await expect(subject).toHaveValue("Plans for Friday");

    // × with unsaved changes: the shared prompt.
    await composer.locator('[data-part="window.close"]').click();
    const prompt = unsavedPrompt(page);
    await expect(prompt).toBeVisible();
    await expect(prompt).toContainText("Save changes to “Plans for Friday”?");
    await expectDialogFooter(prompt, {
      os,
      roles: ["primary", "secondary", "cancel"],
      focused: "primary",
    });
    await expect(footerButton(prompt, "primary")).toHaveText("Save draft");
    await prompt.getByRole("button", { name: "Cancel" }).click();
    await expect(prompt).toBeHidden();
    await expect(subject).toHaveValue("Plans for Friday");

    // A saved draft's Discard asks; Enter on the focused Keep editing discards nothing.
    await composer.getByRole("button", { name: "Save draft", exact: true }).click();
    await expect(composer.locator('[data-part="composer.saved"]')).toBeVisible();
    await composer.getByRole("button", { name: "Discard", exact: true }).click();
    const discard = page.locator('[data-part="window"][data-window^="composer-discard"]');
    await expect(discard).toBeVisible();
    await expectDialogFooter(discard, { os, roles: ["primary", "cancel"], focused: "cancel" });
    await expect(footerButton(discard, "primary")).toHaveText("Discard draft");
    await expect(footerButton(discard, "primary")).toHaveAttribute("data-variant", "danger");
    await page.keyboard.press("Enter");
    await expect(discard).toBeHidden();
    await expect(composer).toBeVisible();
  });

  test(`print, snooze and the message source follow the pattern (${os})`, async ({ page }) => {
    await openAs(page, os);
    await messageRows(page).filter({ hasText: SUBJECT }).click();

    await page.keyboard.press("Control+p");
    const print = page.getByRole("dialog", { name: "Print" });
    await expect(print).toBeVisible();
    await expectDialogFooter(print, { os, roles: ["primary", "secondary", "cancel"] });
    await expect(footerButton(print, "primary")).toHaveText("Print");
    await expect(footerButton(print, "secondary")).toHaveText("Save as PDF…");
    await print.getByRole("button", { name: "Cancel" }).click();
    await expect(print).toBeHidden();

    await readerAction(page, "Snooze…");
    const snooze = page.getByRole("dialog", { name: "Snooze" });
    await expect(snooze).toBeVisible();
    await expectDialogFooter(snooze, { os, roles: ["primary", "cancel"] });
    await expect(footerButton(snooze, "primary")).toHaveText("Snooze");
    await snooze.getByRole("button", { name: "Cancel" }).click();
    await expect(snooze).toBeHidden();

    // The source is a tool window: Copy all, and × closes it.
    await messageRows(page).filter({ hasText: SUBJECT }).click();
    await page.keyboard.press("Control+u");
    const source = page.locator('[data-part="window"][data-window^="message-source:"]');
    await expect(source).toBeVisible();
    await expect(source.locator('[data-part="dialog.footer"]')).toHaveCount(0);
    await expect(source.getByRole("button", { name: "Copy all" })).toBeVisible();
    await source.locator('[data-part="window.close"]').click();
    await expect(source).toHaveCount(0);
  });
}

test("folder dialogs: the name asks before it is lost, Delete folder focuses Cancel", async ({
  page,
}) => {
  await emulateViewerOs(page, "windows");
  await page.goto(server.url);
  await showView(page, "Mail");
  const archive = workFolderRow(page, "Archive");
  await expect(archive).toBeVisible({ timeout: 30_000 });

  await folderMenuItem(page, archive, "New subfolder…");
  const nameWindow = windowById(page, "folder-name");
  const name = nameWindow.getByRole("textbox", { name: "Name" });
  await expect(name).toBeFocused();
  await expectDialogFooter(nameWindow, { os: "windows", roles: ["primary", "cancel"] });
  await name.fill("Temporary");
  await name.press("Escape");
  const prompt = unsavedPrompt(page);
  await expect(prompt).toContainText("Save changes to “Temporary”?");
  await prompt.getByRole("button", { name: "Save", exact: true }).click();
  await expect(nameWindow).toHaveCount(0);
  await expect(workFolderRow(page, "Temporary")).toBeVisible();

  // Into Trash, then for good after asking: the red verb is never the default.
  await folderMenuItem(page, workFolderRow(page, "Temporary"), "Delete folder");
  await expect
    .poll(async () =>
      (await control<{ name: string }[]>(server, "imap-mailboxes", { account: "work" })).map(
        (m) => m.name,
      ),
    )
    .toContain("Trash/Temporary");
  await folderMenuItem(page, workFolderRow(page, "Temporary"), "Delete folder");
  const confirm = windowById(page, "folder-delete");
  await expect(confirm.getByText("Delete the folder “Temporary” for good?")).toBeVisible();
  await expectDialogFooter(confirm, {
    os: "windows",
    roles: ["primary", "cancel"],
    focused: "cancel",
  });
  await expect(footerButton(confirm, "primary")).toHaveText("Delete folder");
  await page.keyboard.press("Enter");
  await expect(confirm).toHaveCount(0);
  await expect(workFolderRow(page, "Temporary")).toBeVisible();
});

test("the move question focuses Move; Esc goes back to the picker", async ({ page }) => {
  await openAs(page, "windows");
  await messageRows(page).filter({ hasText: SUBJECT }).click();
  await messageList(page).focus();
  await page.keyboard.press("v");
  const move = page.getByRole("dialog", { name: "Move to folder" });
  const filter = move.getByRole("combobox", { name: "Find a folder" });
  await expect(filter).toBeFocused();
  await page.keyboard.type("trash");
  await page.keyboard.press("Enter");
  const question = move.locator('[data-part="dialog.confirm"][data-confirm="move"]');
  await expect(question).toBeVisible();
  await expectDialogFooter(move, {
    os: "windows",
    roles: ["primary", "cancel"],
    focused: "primary",
  });
  await page.keyboard.press("Escape");
  await expect(question).toHaveCount(0);
  await expect(move).toBeVisible();
  await expect(filter).toBeFocused();
});

test("Gmail: Delete label focuses Cancel, Apply labels saves or asks", async ({ page }) => {
  await openAs(page, "windows");
  await syncAccount(page, "Gmail", 4);
  const labels = page.locator('[data-part="label-list"]');
  await labels.getByRole("button", { name: "Delete label Work/Projects" }).click();
  const remove = windowById(page, "gmail-label-delete");
  await expectDialogFooter(remove, {
    os: "windows",
    roles: ["primary", "cancel"],
    focused: "cancel",
  });
  await expect(footerButton(remove, "primary")).toHaveText("Delete label");
  await page.keyboard.press("Escape");
  await expect(remove).toHaveCount(0);

  await messageRows(page).filter({ hasText: "Lunch on Friday?" }).click();
  await readerAction(page, "Labels…");
  const apply = page.getByRole("dialog", { name: "Apply labels" });
  await expectDialogFooter(apply, { os: "windows", roles: ["primary", "cancel"] });
  await expect(footerButton(apply, "primary")).toHaveText("Save");
  await apply.getByRole("checkbox", { name: "Receipts" }).check();
  await apply.getByRole("button", { name: "Cancel" }).click();
  const prompt = unsavedPrompt(page);
  await expect(prompt).toBeVisible();
  await prompt.getByRole("button", { name: "Don't save" }).click();
  await expect(apply).toBeHidden();
});

test("Exchange: the category editor asks, Apply categories saves", async ({ page }) => {
  await openAs(page, "windows");
  await syncAccount(page, "Contoso", 2);
  await page.getByRole("button", { name: "New category" }).click();
  const editor = page.getByRole("dialog", { name: "Category" });
  const name = editor.getByRole("textbox", { name: "Name" });
  await expect(name).toBeFocused();
  await expectDialogFooter(editor, { os: "windows", roles: ["primary", "cancel"] });
  await name.fill("Later");
  await editor.getByRole("button", { name: "Cancel" }).click();
  const prompt = unsavedPrompt(page);
  await expect(prompt).toContainText("Save changes to “Later”?");
  await prompt.getByRole("button", { name: "Don't save" }).click();
  await expect(editor).toBeHidden();

  await messageRows(page).first().click();
  await readerAction(page, "Categories…");
  const apply = page.getByRole("dialog", { name: "Apply categories" });
  await expectDialogFooter(apply, { os: "windows", roles: ["primary", "cancel"] });
  await expect(footerButton(apply, "primary")).toHaveText("Save");
  await apply.getByRole("button", { name: "Cancel" }).click();
  await expect(apply).toBeHidden();
});
