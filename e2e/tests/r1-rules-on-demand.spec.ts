import { expect, test, type Locator, type Page } from "@playwright/test";
import { nailPerTest, screenshotFile, serverMessages } from "../support/nail";
import { newRuleFromText, openRulesWindow } from "../support/rules";
import { accountSectionNamed, messageRow, messageRows, sectionInbox } from "../support/views";

/**
 * R1 (ADR 0219): rules run on demand against the `demo-multi` "Work"
 * account's mock IMAP server — a rule just written runs on the selected
 * messages from the rules window, "Run rule" in the message-list row menu
 * and in the folder menu runs a rule on the row / the folder, and every
 * click is undone as one; switching a new rule's editor to Text and back
 * shows no error.
 */

const server = nailPerTest({ seed: "demo-multi" });

const WELCOME = "Welcome to Nail";
const RELEASE = "Release notes for M1";

async function openWorkInbox(page: Page): Promise<void> {
  await page.goto(server.url);
  await sectionInbox(accountSectionNamed(page, "Work")).click();
  await expect(messageRows(page)).toHaveCount(15, { timeout: 30_000 });
}

async function flagged(subject: string): Promise<boolean | null> {
  const found = (await serverMessages(server, "INBOX")).find((m) => m.subject.includes(subject));
  return found ? found.flags.includes("\\Flagged") : null;
}

/** Saves `text` as a new rule from the text view. */
async function saveRuleText(window: Locator, text: string): Promise<Locator> {
  const editor = await newRuleFromText(window, text);
  await editor.getByRole("button", { name: "Save rule" }).click();
  await expect(editor.getByText("The rule is saved.")).toBeVisible();
  return editor;
}

const FLAG_ALL = `rule "Flag them" trigger="manual" {
    then {
        flag
    }
}
`;

const FLAG_RELEASES = `rule "Flag releases" trigger="manual" {
    when {
        subject contains="Release notes"
    }
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
  const views = editor.locator('[data-part="rules.views"]');
  const before = await editor.getByRole("button", { name: "Save rule" }).boundingBox();
  await editor.getByRole("button", { name: "Text" }).click();
  await expect(editor.getByRole("textbox", { name: "The rule in rules.kdl syntax" })).toHaveValue(
    /rule "Fresh"/,
  );
  await editor.getByRole("button", { name: "Editor" }).click();
  await expect(editor.getByRole("textbox", { name: "Name" })).toHaveValue("Fresh");
  await expect(editor.getByRole("alert")).toHaveCount(0);
  // What is still missing is a compact indicator beside the switch: the
  // buttons below stay where they were.
  await expect(views.getByRole("button", { name: "One thing to fix" })).toBeVisible();
  expect(await editor.getByRole("button", { name: "Save rule" }).boundingBox()).toEqual(before);
});

test("a rule just written runs on the selected messages and is undone", async ({ page }) => {
  await openWorkInbox(page);
  await messageRow(page, WELCOME).click();
  await messageRow(page, RELEASE).click({ modifiers: ["Control"] });
  const window = await openRulesWindow(page);
  await saveRuleText(window, FLAG_ALL);

  const panel = window.getByRole("region", { name: "Run rules" });
  await expect(panel.getByText("Runs “Flag them”.")).toBeVisible();
  await panel.getByRole("radio", { name: "The selected messages (2)" }).check();
  await page.screenshot({ path: screenshotFile("r1-rules", "run-panel") });
  await panel.getByRole("button", { name: "Run now" }).click();
  await expect(panel.getByRole("status")).toHaveText(
    "The rule ran: 2 messages matched, 2 changes queued.",
  );
  await expect.poll(() => flagged(WELCOME)).toBe(true);
  await expect.poll(() => flagged(RELEASE)).toBe(true);
  // Only the selection: other messages stay as they were.
  expect(await flagged("Notes for Monday")).not.toBe(true);

  // The run is in the rule's history; Undo takes back the whole click.
  const runs = window.getByRole("region", { name: "Recent runs" });
  await expect(runs.getByText(/^By hand: 2 messages, done$/)).toBeVisible();
  await panel.getByRole("button", { name: "Undo" }).click();
  await expect(panel.getByRole("status")).toHaveText("Everything the run did was undone.");
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
