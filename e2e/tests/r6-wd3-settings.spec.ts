import { expect, test, type Page } from "@playwright/test";
import { axeViolations } from "../support/axe";
import { nailPerTest } from "../support/nail";
import { rpcOk } from "../support/remote";
import {
  answerQuestion,
  discardSettings,
  saveSettings,
  settingsQuestion,
  settingsSaveBar,
} from "../support/settings";
import { openMail, openSettings, runFromPalette, settingsNav, windowById } from "../support/views";
import { emulateViewerOs, expectDialogFooter, type ViewerOs } from "../support/windowTour";

/**
 * Settings and accounts in the one dialog pattern (R6 WD3, ADR 0239)
 * against `nail serve --seed demo-multi`:
 *
 * - nothing in a section applies before the pinned bar's Save; Discard
 *   takes the change back; leaving the section asks with the shared prompt;
 * - "Remove account…" asks with a destructive confirmation (Cancel
 *   focused, Enter never removes);
 * - the Add account wizard: Cancel on every step, the unsaved-changes
 *   question once something was typed.
 *
 * Each runs as Windows and as macOS (the footers mirror).
 */

const server = nailPerTest({ seed: "demo-multi" });

const unsavedPrompt = (page: Page) =>
  page.locator('[data-part="window"][data-window^="unsaved-changes:"]');

/** The page as a viewer on `os`, with the mail view loaded. */
async function openAs(page: Page, os: ViewerOs): Promise<void> {
  await emulateViewerOs(page, os);
  await openMail(page, server.url);
}

/** Whether the core's new-mail notifications are on. */
async function notificationsEnabled(): Promise<boolean> {
  const answer = await rpcOk(server, { type: "GetNotificationSettings" }, "NotificationSettings");
  return (answer.settings as { enabled: boolean }).enabled;
}

for (const os of ["windows", "macos"] as const) {
  test(`a settings section saves only with the bar, and asks when left (${os})`, async ({
    page,
  }) => {
    await openAs(page, os);
    const settings = await openSettings(page, "Notifications");
    const enabled = settings.getByRole("checkbox", { name: "Tell me about new mail" });
    const before = await enabled.isChecked();
    await expect(settingsSaveBar(page)).toBeHidden();

    // A change shows the bar (its footer in the system's order); Discard takes it back.
    await enabled.click();
    const bar = settingsSaveBar(page);
    await expect(bar).toBeVisible();
    await expect(bar).toContainText("Unsaved changes to “Notifications”");
    await expectDialogFooter(bar, { os, roles: ["primary", "cancel"] });
    expect(await axeViolations(page, '[data-part="settings-save-bar"]')).toEqual([]);
    await discardSettings(page);
    await expect(enabled).toBeChecked({ checked: before });

    // Leaving the section with a change asks; Cancel stays.
    await enabled.click();
    const accounts = settingsNav(settings).getByRole("button", { name: "Accounts", exact: true });
    await accounts.click();
    const prompt = unsavedPrompt(page);
    await expect(prompt).toBeVisible();
    await expect(prompt).toContainText("Save changes to “Notifications”?");
    await expectDialogFooter(prompt, {
      os,
      roles: ["primary", "secondary", "cancel"],
      focused: "primary",
    });
    await prompt.getByRole("button", { name: "Cancel" }).click();
    await expect(prompt).toBeHidden();
    await expect(settings.locator('[data-part="settings-page.title"]')).toHaveText("Notifications");
    await expect(enabled).toBeChecked({ checked: !before });

    // Save in the prompt writes the change, then the other section shows.
    await accounts.click();
    await unsavedPrompt(page).getByRole("button", { name: "Save", exact: true }).click();
    await expect(settings.locator('[data-part="settings-page.title"]')).toHaveText("Accounts");
    await expect.poll(() => notificationsEnabled()).toBe(!before);

    // Back, then the bar's Save puts it back as it was.
    await openSettings(page, "Notifications");
    await enabled.click();
    await saveSettings(page);
    await expect.poll(() => notificationsEnabled()).toBe(before);
  });

  test(`Remove account… asks, with Cancel as the default (${os})`, async ({ page }) => {
    await openAs(page, os);
    const settings = await openSettings(page, "Accounts");
    const remove = settings.getByRole("button", { name: "Remove account…" });
    await remove.click();
    const question = settingsQuestion(page);
    await expect(question).toBeVisible();
    await expect(question).toContainText(/Remove the account “.+”\?/);
    await expectDialogFooter(question, { os, roles: ["primary", "cancel"], focused: "cancel" });
    await expect(question.locator('button[data-role="primary"]')).toHaveAttribute(
      "data-variant",
      "danger",
    );
    expect(
      await axeViolations(page, '[data-part="window"]:has([data-part="dialog.confirm"])'),
    ).toEqual([]);
    // Enter on the focused Cancel: nothing is removed.
    await page.keyboard.press("Enter");
    await expect(question).toBeHidden();
    await expect(settings.getByRole("textbox", { name: "Account name" })).toBeVisible();
  });

  test(`Add account: Cancel on every step, typed text asks (${os})`, async ({ page }) => {
    await openAs(page, os);
    await runFromPalette(page, "add account", "Add account");
    const dialog = windowById(page, "account-setup");
    await expect(dialog).toBeVisible();
    await expectDialogFooter(dialog, { os, roles: ["primary", "cancel"] });

    await dialog.getByRole("textbox", { name: "Email address" }).fill("bob@elsewhere.example");
    await dialog.getByRole("button", { name: "Continue" }).click();
    await expect(dialog.getByRole("group", { name: "Incoming mail" })).toBeVisible();
    await expectDialogFooter(dialog, { os, roles: ["primary", "secondary", "cancel"] });

    // Half an account cannot be saved: the prompt offers to discard it.
    await page.keyboard.press("Escape");
    const prompt = unsavedPrompt(page);
    await expect(prompt).toBeVisible();
    await expect(prompt.getByRole("button", { name: "Keep editing" })).toBeVisible();
    await prompt.getByRole("button", { name: "Keep editing" }).click();
    await expect(prompt).toBeHidden();
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Cancel" }).click();
    await answerPrompt(page, "Discard changes");
    await expect(dialog).toBeHidden();
  });
}

/** Answers the open unsaved-changes prompt with `action`. */
async function answerPrompt(page: Page, action: string): Promise<void> {
  const prompt = unsavedPrompt(page);
  await prompt.getByRole("button", { name: action, exact: true }).click();
  await expect(prompt).toBeHidden();
}

test("Reset the filter… asks before the junk filter forgets", async ({ page }) => {
  await openAs(page, "windows");
  const settings = await openSettings(page, "Mail & junk");
  await settings.getByRole("button", { name: "Reset the filter…" }).click();
  const question = settingsQuestion(page);
  await expect(question).toContainText("Reset the junk filter?");
  await expectDialogFooter(question, {
    os: "windows",
    roles: ["primary", "cancel"],
    focused: "cancel",
  });
  await answerQuestion(page, "Reset filter");
  await expect(settings.getByText("The filter forgot everything it learned.")).toBeVisible();
});
