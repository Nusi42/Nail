import { expect, test, type Locator } from "@playwright/test";
import { axeViolations } from "../support/axe";
import { nailPerTest, screenshotFile } from "../support/nail";
import {
  FLAG_RELEASES,
  RELEASE,
  WELCOME,
  flaggedOnServer,
  newRuleFromText,
  openRulesWindow,
  openWorkInbox,
  ruleFromMessageRow,
  ruleInList,
  rulesFooter,
  rulesList,
  saveRule,
} from "../support/rules";
import { messageRow } from "../support/views";

/**
 * R3 (ADR 0227): the rules window's list as a multi-select listbox — two
 * rules selected with Ctrl+click run together on the selected messages
 * ("Run 2 rules", one undo for both), the keyboard model (Shift+arrows,
 * Escape, Ctrl+A) — and "Values from this mail" right after a condition's
 * value field, against the `demo-multi` "Work" account's mock IMAP server.
 * axe checks the list with two rules selected (`aria-multiselectable`).
 * The combined Exchange badge is in `exchange.spec.ts`. Screenshots in
 * `<build dir>/screens/r3-rules/`.
 */

const server = nailPerTest({ seed: "demo-multi" });

const flagged = (subject: string) => flaggedOnServer(server, subject);

/** A manual rule that flags the welcome message, as `rules.kdl` text. */
const FLAG_WELCOME = `rule "Flag welcome" trigger="manual" {
    when {
        subject contains="Welcome"
    }
    then {
        flag
    }
}
`;

/** Saves `text` as a new rule from the text view; returns the editor. */
async function saveRuleText(window: Locator, text: string): Promise<Locator> {
  const editor = await newRuleFromText(window, text);
  await saveRule(editor);
  return editor;
}

/** Whether the rule `name`'s option is selected. */
const selected = (window: Locator, name: string) =>
  expect(ruleInList(window, name)).toHaveAttribute("aria-selected", "true");
const notSelected = (window: Locator, name: string) =>
  expect(ruleInList(window, name)).toHaveAttribute("aria-selected", "false");

test("two selected rules run together on the selected messages and are undone", async ({
  page,
}) => {
  await openWorkInbox(page, server, { width: 1600, height: 1000 });
  await messageRow(page, WELCOME).click();
  await messageRow(page, RELEASE).click({ modifiers: ["Control"] });
  const window = await openRulesWindow(page);
  await saveRuleText(window, FLAG_RELEASES);
  await saveRuleText(window, FLAG_WELCOME);
  await expect(rulesList(window)).toHaveAttribute("aria-multiselectable", "true");

  // Ctrl+click adds "Flag releases"; the editor shows the rule clicked last.
  await ruleInList(window, "Flag releases").click({ modifiers: ["Control"] });
  await selected(window, "Flag releases");
  await selected(window, "Flag welcome");
  const editor = window.getByRole("form", { name: "Rule" });
  await expect(editor.getByRole("textbox", { name: "Name" })).toHaveValue("Flag releases");
  const footer = rulesFooter(editor);
  await footer
    .getByRole("combobox", { name: "Run on" })
    .selectOption({ label: "Selected messages (2)" });
  const run = footer.getByRole("button", { name: "Run 2 rules" });
  await expect(run).toBeInViewport();
  await page.screenshot({ path: screenshotFile("r3-rules", "two-selected") });
  await run.click();
  await expect(footer.getByRole("status")).toHaveText(/^2 rules ran: 2 messages matched/);
  // Each rule acted on its own message.
  await expect.poll(() => flagged(RELEASE)).toBe(true);
  await expect.poll(() => flagged(WELCOME)).toBe(true);
  expect(await flagged("Notes for Monday")).not.toBe(true);

  // One Undo takes back what both rules did.
  await footer.getByRole("button", { name: "Undo", exact: true }).click();
  await expect(footer.getByRole("status")).toHaveText("Everything the run did was undone.");
  await expect.poll(() => flagged(RELEASE)).toBe(false);
  await expect.poll(() => flagged(WELCOME)).toBe(false);
});

test("the rule list's keyboard model", async ({ page }) => {
  await openWorkInbox(page, server, { width: 1600, height: 1000 });
  const window = await openRulesWindow(page);
  await saveRuleText(window, FLAG_RELEASES);
  await saveRuleText(window, FLAG_WELCOME);
  const footer = rulesFooter(window.getByRole("form", { name: "Rule" }));
  // The new rule is the one selected and focused.
  await ruleInList(window, "Flag welcome").click();
  await expect(ruleInList(window, "Flag welcome")).toBeFocused();
  await page.keyboard.press("Shift+ArrowUp");
  await selected(window, "Flag releases");
  await selected(window, "Flag welcome");
  await expect(ruleInList(window, "Flag releases")).toBeFocused();
  await expect(footer.getByRole("button", { name: "Run 2 rules" })).toBeVisible();
  // The listbox with two rules selected passes axe.
  expect(await axeViolations(page, '[data-part="rules"]'), "rule list, two selected").toEqual([]);
  // Escape goes back to the focused rule, and the window stays open.
  await page.keyboard.press("Escape");
  await notSelected(window, "Flag welcome");
  await selected(window, "Flag releases");
  await expect(window).toBeVisible();
  await expect(footer.getByRole("button", { name: "Run", exact: true })).toBeVisible();
  // Ctrl+A selects every rule; a plain arrow selects only the next one.
  await page.keyboard.press("Control+a");
  await selected(window, "Flag welcome");
  await page.keyboard.press("ArrowDown");
  await notSelected(window, "Flag releases");
  await selected(window, "Flag welcome");
  await expect(ruleInList(window, "Flag welcome")).toBeFocused();
});

test("Values from this mail sits right after the condition's value", async ({ page }) => {
  await openWorkInbox(page, server, { width: 1600, height: 1000 });
  const editor = await ruleFromMessageRow(page, WELCOME);
  const row = editor.locator('[data-part="rules.condition"]').first();
  const values = row.getByRole("button", { name: "Values from this mail" });
  await expect(values).toBeVisible();
  const [field, button, match] = await Promise.all([
    row.getByRole("combobox", { name: "Text" }).boundingBox(),
    values.boundingBox(),
    row.getByRole("checkbox", { name: "Match upper and lower case" }).boundingBox(),
  ]);
  type Box = NonNullable<typeof field>;
  /** `b` comes after `a` in reading order: to its right on its line, or on a later line. */
  const after = (a: Box, b: Box) =>
    b.y >= a.y + a.height - 1 || (b.y < a.y + a.height && b.x >= a.x + a.width - 1);
  expect(field && button && match).toBeTruthy();
  if (field && button && match) {
    // The button right beside the value, on its line.
    expect(button.x).toBeGreaterThanOrEqual(field.x + field.width - 1);
    expect(button.y).toBeLessThan(field.y + field.height);
    // The case check box after the button (the row may wrap before it).
    expect(after(button, match)).toBe(true);
  }
  await page.screenshot({ path: screenshotFile("r3-rules", "values-button") });
});
