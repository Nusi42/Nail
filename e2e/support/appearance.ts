import type { Locator } from "@playwright/test";
import { saveSettings } from "./settings";

/**
 * Settings → Appearance in the R6 pattern (ADR 0239): nothing applies until
 * the section's "Unsaved changes" bar saves it (`saveSettings`).
 */

/** Picks the theme `name` in the Appearance gallery and saves it. */
export async function pickTheme(appearance: Locator, name: string | RegExp): Promise<void> {
  await appearance
    .getByRole("radiogroup", { name: "Themes" })
    .getByRole("radio", { name: typeof name === "string" ? new RegExp(name) : name })
    .click();
  await saveSettings(appearance.page());
}
