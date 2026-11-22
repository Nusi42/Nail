/**
 * Settings scenarios (R6, ADR 0239): nothing in a settings section applies
 * until the pinned save bar's Save, and actions ask in the shared
 * confirmation window. Specs change settings, then call `saveSettings`;
 * they answer a question with `answerQuestion`.
 */
import { expect, type Locator, type Page } from "@playwright/test";

/** The pinned "Unsaved changes to “…”" bar of the shown settings section. */
export function settingsSaveBar(page: Page): Locator {
  return page.getByRole("group", { name: /^Unsaved changes to “/ });
}

/** Presses the bar's Save and waits until everything is saved (the bar went). */
export async function saveSettings(page: Page): Promise<void> {
  const bar = settingsSaveBar(page);
  await bar.getByRole("button", { name: "Save", exact: true }).click();
  await expect(bar).toBeHidden();
}

/** Presses the bar's Discard. */
export async function discardSettings(page: Page): Promise<void> {
  const bar = settingsSaveBar(page);
  await bar.getByRole("button", { name: "Discard", exact: true }).click();
  await expect(bar).toBeHidden();
}

/** The open question (a modal confirmation window with the shared `ConfirmDialog`). */
export function settingsQuestion(page: Page): Locator {
  return page.locator('[data-part="window"]').filter({
    has: page.locator('[data-part="dialog.confirm"]'),
  });
}

/** Answers the open question with its verb `action` and waits until it closed. */
export async function answerQuestion(page: Page, action: string): Promise<void> {
  const question = settingsQuestion(page);
  await question.getByRole("button", { name: action, exact: true }).click();
  await expect(question).toBeHidden();
}
