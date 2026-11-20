import { expect, type Locator, type Page } from "@playwright/test";
import { runFromPalette } from "./views";

/**
 * The rules window in the rules scenarios (ADR 0024, R1): opening it from
 * the command palette and entering a new rule in its text view.
 */

/** Opens "Filter rules" from the command palette. */
export async function openRulesWindow(page: Page): Promise<Locator> {
  await runFromPalette(page, "filter rules", "Filter rules");
  const window = page.getByRole("dialog", { name: "Filter rules" });
  await expect(window).toBeVisible();
  return window;
}

/** Starts a new rule and enters it in the text view; returns the editor. */
export async function newRuleFromText(window: Locator, text: string): Promise<Locator> {
  await window.getByRole("button", { name: "New rule" }).click();
  const editor = window.getByRole("form", { name: "Rule" });
  await editor.getByRole("button", { name: "Text" }).click();
  await editor.getByRole("textbox", { name: "The rule in rules.kdl syntax" }).fill(text);
  return editor;
}
