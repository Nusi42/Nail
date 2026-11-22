import { expect, type Locator, type Page } from "@playwright/test";

/**
 * Settings → Appearance in the R6 pattern (ADR 0239): nothing applies until
 * the section's "Unsaved changes" bar saves it.
 */

/** Saves the shown settings section with its "Unsaved changes" bar. */
export async function saveSettingsSection(page: Page): Promise<void> {
  const bar = page.locator('[data-part="settings-save-bar"]');
  await bar.locator('button[data-action="save-settings"]').click();
  await expect(bar).toBeHidden();
}

/** Picks the theme `name` in the Appearance gallery and saves it. */
export async function pickTheme(appearance: Locator, name: string | RegExp): Promise<void> {
  await appearance
    .getByRole("radiogroup", { name: "Themes" })
    .getByRole("radio", { name: typeof name === "string" ? new RegExp(name) : name })
    .click();
  await saveSettingsSection(appearance.page());
}
