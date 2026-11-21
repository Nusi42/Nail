import { expect, test, type Locator, type Page } from "@playwright/test";
import { nailPerTest, screenshotFile } from "../support/nail";
import {
  FLAG_RELEASES,
  RELEASE,
  WELCOME,
  flaggedOnServer,
  newRuleFromText,
  openRulesWindow,
  openWorkInbox as openWorkInboxOf,
  rulesFooter,
  saveRule,
} from "../support/rules";
import { accountSectionNamed, messageRow, sectionInbox } from "../support/views";

/**
 * R1 (ADR 0219): rules run on demand against the `demo-multi` "Work"
 * account's mock IMAP server — a rule just written runs on the selected
 * messages from the rules window, "Run rule" in the message-list row menu
 * and in the folder menu runs a rule on the row / the folder, and every
 * click is undone as one; switching a new rule's editor to Text and back
 * shows no error. Review R1: a new rule is saved and run from its editor
 * ("Save and run", in view at 1024×700), and Escape on a problem's details
 * closes only the details. R2: running happens in the editor's footer
 * ("Run on" and Run).
 */

const server = nailPerTest({ seed: "demo-multi" });

const openWorkInbox = (page: Page, size?: { width: number; height: number }) =>
  openWorkInboxOf(page, server, size);
const flagged = (subject: string) => flaggedOnServer(server, subject);

/** Saves `text` as a new rule from the text view. */
async function saveRuleText(window: Locator, text: string): Promise<Locator> {
  const editor = await newRuleFromText(window, text);
  await saveRule(editor);
  return editor;
}

const FLAG_ALL = `rule "Flag them" trigger="manual" {
    then {
        flag
    }
}
`;

test("a new rule switches Editor, Text, Editor without an error", async ({ page }) => {
  await openWorkInbox(page);
  const window = await openRulesWindow(page);
  await window.getByRole("button", { name: "New rule" }).click();
  const editor = window.getByRole("form", { name: "Rule" });
  await editor.getByRole("textbox", { name: "Name" }).fill("Fresh");
  const footer = rulesFooter(editor);
  const before = await footer.getByRole("button", { name: "Save rule" }).boundingBox();
  await editor.getByRole("radio", { name: "Text" }).click();
  await expect(editor.getByRole("textbox", { name: "The rule in rules.kdl syntax" })).toHaveValue(
    /rule "Fresh"/,
  );
  await editor.getByRole("radio", { name: "Editor" }).click();
  await expect(editor.getByRole("textbox", { name: "Name" })).toHaveValue("Fresh");
  await expect(editor.getByRole("alert")).toHaveCount(0);
  // What is still missing is a compact indicator in the footer: the
  // buttons stay where they were.
  const badge = footer.getByRole("button", { name: "One thing to fix" });
  await expect(badge).toBeVisible();
  expect(await footer.getByRole("button", { name: "Save rule" }).boundingBox()).toEqual(before);

  // Escape closes the details only, never the window around them.
  const details = footer.locator('[data-part="problem.details"]');
  await badge.focus();
  await page.keyboard.press("Enter");
  await expect(details).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(details).toBeHidden();
  await expect(window).toBeVisible();
  await expect(badge).toBeFocused();
});

test("a new rule is saved and run from its editor at 1024×700", async ({ page }) => {
  await openWorkInbox(page, { width: 1024, height: 700 });
  await messageRow(page, WELCOME).click();
  const window = await openRulesWindow(page);
  const editor = await newRuleFromText(window, FLAG_ALL);
  const footer = rulesFooter(editor);
  await footer
    .getByRole("combobox", { name: "Run on" })
    .selectOption({ label: "Selected messages (1)" });
  const run = footer.getByRole("button", { name: "Save and run" });
  // In the footer: in view without scrolling the window.
  await expect(run).toBeInViewport();
  const save = await footer.getByRole("button", { name: "Save rule" }).boundingBox();
  await page.screenshot({ path: screenshotFile("r1-rules", "save-and-run-1024") });
  await run.click();
  await expect(footer.getByRole("status")).toHaveText(/^The rule ran: (1|one) message matched/);
  await expect.poll(() => flagged(WELCOME)).toBe(true);
  expect(await flagged(RELEASE)).not.toBe(true);
  // Saved: the list shows it, and the next click runs it as saved.
  await expect(window.getByRole("button", { name: "Flag them", exact: true })).toBeVisible();
  await expect(footer.getByRole("button", { name: "Run", exact: true })).toBeVisible();
  // What the run did moved nothing.
  expect(await footer.getByRole("button", { name: "Save rule" }).boundingBox()).toEqual(save);
});

test("a rule just written runs on the selected messages and is undone", async ({ page }) => {
  await openWorkInbox(page);
  await messageRow(page, WELCOME).click();
  await messageRow(page, RELEASE).click({ modifiers: ["Control"] });
  const window = await openRulesWindow(page);
  const editor = await saveRuleText(window, FLAG_ALL);

  const footer = rulesFooter(editor);
  await footer
    .getByRole("combobox", { name: "Run on" })
    .selectOption({ label: "Selected messages (2)" });
  await page.screenshot({ path: screenshotFile("r1-rules", "run-panel") });
  await footer.getByRole("button", { name: "Run", exact: true }).click();
  await expect(footer.getByRole("status")).toHaveText(
    "The rule ran: 2 messages matched, 2 changes queued.",
  );
  await expect.poll(() => flagged(WELCOME)).toBe(true);
  await expect.poll(() => flagged(RELEASE)).toBe(true);
  // Only the selection: other messages stay as they were.
  expect(await flagged("Notes for Monday")).not.toBe(true);

  // The run is in the rule's history; Undo takes back the whole click.
  await editor.getByRole("tab", { name: /^Recent runs/ }).click();
  const runs = window.getByRole("region", { name: "Recent runs" });
  await expect(runs.getByText(/^By hand: 2 messages, done$/)).toBeVisible();
  await expect(runs.getByText("2 selected messages")).toBeVisible();
  await footer.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(footer.getByRole("status")).toHaveText("Everything the run did was undone.");
  await expect.poll(() => flagged(WELCOME)).toBe(false);
  await expect.poll(() => flagged(RELEASE)).toBe(false);
});

test("Run rule in the row menu and the folder menu, undone from the toast", async ({ page }) => {
  await openWorkInbox(page);
  const window = await openRulesWindow(page);
  await saveRuleText(window, FLAG_ALL);
  await saveRuleText(window, FLAG_RELEASES);
  await window.getByRole("button", { name: "Close" }).first().click();
  await expect(window).toBeHidden();

  // The row menu: one rule on the row.
  await messageRow(page, WELCOME).click({ button: "right" });
  await page.getByRole("menu").getByRole("menuitem", { name: "Run rule" }).click();
  const rulesMenu = page.getByRole("menu");
  await expect(rulesMenu.getByRole("menuitem", { name: "All rules that are on" })).toBeVisible();
  await page.screenshot({ path: screenshotFile("r1-rules", "row-menu") });
  await rulesMenu.getByRole("menuitem", { name: "Flag them" }).click();
  const toast = page.locator('[data-part="mail-actions.toast"][data-source="rules"]');
  await expect(toast).toHaveText(/The rule ran: 1 message matched|The rule ran: one message/);
  await expect.poll(() => flagged(WELCOME)).toBe(true);
  expect(await flagged(RELEASE)).toBe(false);
  await toast.getByRole("button", { name: "Undo" }).click();
  await expect(toast).toContainText("Everything the run did was undone.");
  await expect.poll(() => flagged(WELCOME)).toBe(false);

  // The folder menu: the folder's messages.
  await sectionInbox(accountSectionNamed(page, "Work")).click({ button: "right" });
  await page.getByRole("menu").getByRole("menuitem", { name: "Run rule" }).click();
  await page.getByRole("menu").getByRole("menuitem", { name: "Flag releases" }).click();
  await expect(toast).toContainText("The rule ran");
  await expect.poll(() => flagged(RELEASE)).toBe(true);
  expect(await flagged(WELCOME)).toBe(false);
  await toast.getByRole("button", { name: "Dismiss" }).click();
  await expect(toast).toHaveCount(0);
});
