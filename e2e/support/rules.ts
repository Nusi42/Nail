import { expect, type Locator, type Page } from "@playwright/test";
import { serverMessages, type NailServer } from "./nail";
import { accountSectionNamed, messageRows, runFromPalette, sectionInbox } from "./views";

/** Subjects of two `demo-multi` Work Inbox messages the rules scenarios act on. */
export const WELCOME = "Welcome to Nail";
export const RELEASE = "Release notes for M1";

/** A manual rule that flags the release notes, as `rules.kdl` text. */
export const FLAG_RELEASES = `rule "Flag releases" trigger="manual" {
    when {
        subject contains="Release notes"
    }
    then {
        flag
    }
}
`;

/** How many messages the `demo-multi` "Work" Inbox lists. */
export const WORK_INBOX_ROWS = 15;

/** Opens the `demo-multi` "Work" account's Inbox, at `size` when given. */
export async function openWorkInbox(
  page: Page,
  server: NailServer,
  size?: { width: number; height: number },
): Promise<void> {
  if (size) await page.setViewportSize(size);
  await page.goto(server.url);
  await sectionInbox(accountSectionNamed(page, "Work")).click();
  await expect(messageRows(page)).toHaveCount(WORK_INBOX_ROWS, { timeout: 30_000 });
}

/** Whether the Inbox message whose subject contains `subject` is flagged on the server (`null`: not there). */
export async function flaggedOnServer(
  server: NailServer,
  subject: string,
): Promise<boolean | null> {
  const found = (await serverMessages(server, "INBOX")).find((m) => m.subject.includes(subject));
  return found ? found.flags.includes("\\Flagged") : null;
}

/**
 * The rules window in the rules scenarios (ADR 0024, R1, R2): opening it
 * from the command palette, entering a new rule in its text view, the
 * editor's footer, and the "+ Add …" menus.
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
  await editor.getByRole("radio", { name: "Text" }).click();
  await editor.getByRole("textbox", { name: "The rule in rules.kdl syntax" }).fill(text);
  return editor;
}

/** The editor's footer (status, Delete…, Run on, Run, Test, Save; always in view). */
export function rulesFooter(editor: Locator): Locator {
  return editor.locator('[data-part="rules.footer"]');
}

/** Saves the editor's rule and waits for "Saved". */
export async function saveRule(editor: Locator): Promise<void> {
  const footer = rulesFooter(editor);
  await footer.getByRole("button", { name: "Save rule" }).click();
  await expect(footer.getByText("Saved")).toBeVisible();
}

/** Picks `item` in the menu of the first "+ Add …" button named `button` (conditions, actions). */
export async function addFromMenu(scope: Locator, button: string, item: string): Promise<void> {
  await scope.getByRole("button", { name: button }).first().click();
  await scope.page().getByRole("menuitem", { name: item }).click();
}

/** The toast of a rule run started from a menu. */
export const rulesToast = (page: Page) =>
  page.locator('[data-part="mail-actions.toast"][data-source="rules"]');

/**
 * "Create rule from message" from the row menu of the message whose subject
 * contains `subject`; returns the rules window's editor (its origin is that mail).
 */
export async function ruleFromMessageRow(page: Page, subject: string): Promise<Locator> {
  await messageRows(page).filter({ hasText: subject }).click({ button: "right" });
  await page.getByRole("menu").getByRole("menuitem", { name: "Create rule from message" }).click();
  return page.getByRole("dialog", { name: "Filter rules" }).getByRole("form", { name: "Rule" });
}
