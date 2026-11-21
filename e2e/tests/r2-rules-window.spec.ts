import { expect, test, type Locator, type Page } from "@playwright/test";
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
  rulesFooter,
  saveRule,
} from "../support/rules";
import {
  accountSectionNamed,
  openSettings,
  openSettingsTab,
  sectionInbox,
  showView,
} from "../support/views";

/**
 * R2 (ADR 0222): the redesigned rules window against the `demo-multi`
 * "Work" account's mock IMAP server, at the standard sizes 1600×1000 and
 * 1024×700 — the footer (Run on, Run, Test, Save) always in view, Test
 * opening the Preview tab with the focus on its summary, "Apply to these
 * N", the Run on targets, Delete… asking first, unsaved changes asking on
 * Escape, Editor ↔ Text on an unfinished new rule, and the list going over
 * the editor in a narrow window, and a rule from a mail in a dark theme.
 * Screenshots in `<build dir>/screens/r2-rules/`.
 */

const server = nailPerTest({ seed: "demo-multi" });

const SIZES = [
  { width: 1600, height: 1000 },
  { width: 1024, height: 700 },
] as const;

/** The rule list's buttons (one per rule). */
function ruleInList(window: Locator, name: string): Locator {
  return window
    .getByRole("list", { name: "Rules, in the order they run" })
    .getByRole("button", { name, exact: true });
}

/** Opens the Work Inbox at `size` and the rules window with a saved "Flag releases". */
async function withSavedRule(page: Page, size: { width: number; height: number }) {
  await openWorkInbox(page, server, size);
  const window = await openRulesWindow(page);
  const editor = await newRuleFromText(window, FLAG_RELEASES);
  await saveRule(editor);
  return { window, editor, footer: rulesFooter(editor) };
}

for (const size of SIZES) {
  const at = `${String(size.width)}×${String(size.height)}`;

  test(`footer, preview, Apply to these N and Run on at ${at}`, async ({ page }) => {
    const { window, editor, footer } = await withSavedRule(page, size);
    // The footer is in view without scrolling the window.
    for (const name of ["Run", "Test rule", "Save rule", "Delete rule…"]) {
      await expect(footer.getByRole("button", { name, exact: true })).toBeInViewport();
    }
    const runOn = footer.getByRole("combobox", { name: "Run on" });
    await expect(runOn.getByRole("option")).toHaveText([
      "Selected messages",
      "Current folder (Inbox)",
      "Where the rule applies",
    ]);
    await runOn.selectOption({ label: "Current folder (Inbox)" });

    // Test opens Preview and puts the focus on its summary.
    await editor.getByRole("tab", { name: /^Recent runs/ }).click();
    await footer.getByRole("button", { name: "Test rule" }).click();
    await expect(editor.getByRole("tab", { name: "Preview" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    const results = editor.getByRole("region", { name: "Test results" });
    const summary = results.getByRole("status");
    await expect(summary).toHaveText(/^One message of \d+ matches \(Current folder \(Inbox\)\)\.$/);
    await expect(summary).toBeFocused();
    await expect(results.getByRole("list", { name: "What would happen" })).toHaveText(
      "Add the flag",
    );
    await page.screenshot({
      path: screenshotFile("r2-rules", `preview-${String(size.width)}`),
    });

    // "Apply to this message" runs the rule on exactly the previewed message.
    await results.getByRole("button", { name: "Apply to this message" }).click();
    await expect(footer.getByRole("status")).toHaveText(/^The rule ran: (1|one) message matched/);
    await expect.poll(() => flaggedOnServer(server, RELEASE)).toBe(true);
    expect(await flaggedOnServer(server, WELCOME)).not.toBe(true);
    await expect(editor.getByRole("tab", { name: "Recent runs (1)" })).toBeVisible();
    await expect(window).toBeVisible();
  });

  test(`Delete… and unsaved changes ask inside the window at ${at}`, async ({ page }) => {
    const { window, editor, footer } = await withSavedRule(page, size);

    // Delete… asks first; Cancel keeps the rule.
    await footer.getByRole("button", { name: "Delete rule…" }).click();
    const question = window.getByRole("alertdialog");
    await expect(question).toContainText("Delete the rule “Flag releases”?");
    await expect(question.getByRole("button", { name: "Cancel" })).toBeFocused();
    await page.screenshot({ path: screenshotFile("r2-rules", `delete-${String(size.width)}`) });
    // Answered from the keyboard, the focus goes back to the button that asked.
    await page.keyboard.press("Enter");
    await expect(ruleInList(window, "Flag releases")).toBeVisible();
    await expect(footer.getByRole("button", { name: "Delete rule…" })).toBeFocused();

    // Unsaved changes: Escape asks instead of closing the window.
    const name = editor.getByRole("textbox", { name: "Name" });
    await name.fill("Flag release notes");
    await page.keyboard.press("Escape");
    await expect(question).toContainText("Discard your changes to “Flag release notes”?");
    await expect(window).toBeVisible();
    // Tab stays inside the question (modal): never the window's title bar.
    const keep = question.getByRole("button", { name: "Keep editing" });
    const discard = question.getByRole("button", { name: "Discard changes" });
    await expect(keep).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(discard).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(keep).toBeFocused();
    await page.keyboard.press("Enter");
    await expect(name).toHaveValue("Flag release notes");
    // Back in the field Escape was pressed in.
    await expect(name).toBeFocused();
    await page.keyboard.press("Escape");
    await question.getByRole("button", { name: "Discard changes" }).click();
    await expect(window).toBeHidden();

    // Back again, the rule is as saved; Delete… then deletes it.
    const reopened = await openRulesWindow(page);
    await ruleInList(reopened, "Flag releases").click();
    const again = reopened.getByRole("form", { name: "Rule" });
    await rulesFooter(again).getByRole("button", { name: "Delete rule…" }).click();
    await reopened
      .getByRole("alertdialog")
      .getByRole("button", { name: "Delete the rule" })
      .click();
    await expect(ruleInList(reopened, "Flag releases")).toHaveCount(0);
  });
}

test("Editor ↔ Text keeps an unfinished new rule and highlights its text", async ({ page }) => {
  await openWorkInbox(page, server, SIZES[0]);
  const window = await openRulesWindow(page);
  await window.getByRole("button", { name: "New rule" }).click();
  const editor = window.getByRole("form", { name: "Rule" });
  // A radio group: the arrow keys switch.
  await editor.getByRole("radio", { name: "Editor" }).focus();
  await page.keyboard.press("ArrowRight");
  const source = editor.getByRole("textbox", { name: "The rule in rules.kdl syntax" });
  await expect(source).toBeVisible();
  // No name yet, a value missing: it still goes back to the editor.
  await source.fill(
    'rule "" {\n    when {\n        subject contains=""\n    }\n    then {\n        flag\n    }\n}\n',
  );
  await expect(
    editor.locator('[data-part="rules.text.token"][data-kind="node"]').first(),
  ).toHaveText("rule");
  await expect(editor.locator('[data-part="rules.text.line"]')).toHaveCount(9);
  await editor.getByRole("radio", { name: "Editor" }).click();
  await expect(editor.getByRole("textbox", { name: "Name" })).toHaveValue("");
  await expect(editor.getByRole("textbox", { name: "Name" })).toHaveAttribute(
    "aria-invalid",
    "true",
  );
  await expect(editor.locator('[data-part="rules.condition"]')).toHaveCount(1);
  await expect(rulesFooter(editor).getByRole("button", { name: "2 things to fix" })).toBeVisible();
  await page.screenshot({ path: screenshotFile("r2-rules", "unfinished-rule") });
});

test("a narrow window puts the rule list over the editor", async ({ page }) => {
  await openWorkInbox(page, server, SIZES[1]);
  // The window fits the page: narrower than 760 px, the list goes over the editor.
  await page.setViewportSize({ width: 720, height: 700 });
  const window = await openRulesWindow(page);
  await window.getByRole("button", { name: "New rule" }).click();
  const editor = window.getByRole("form", { name: "Rule" });
  const list = window.locator('[data-part="rules.side"]');
  const [listBox, editorBox] = await Promise.all([list.boundingBox(), editor.boundingBox()]);
  expect(listBox && editorBox && listBox.y + listBox.height <= editorBox.y + 1).toBe(true);
  await expect(rulesFooter(editor).getByRole("button", { name: "Save rule" })).toBeInViewport();
  await page.screenshot({ path: screenshotFile("r2-rules", "stacked-720") });
});

test("Settings shows the rules with the window's look", async ({ page }) => {
  const { window, editor } = await withSavedRule(page, SIZES[1]);
  await editor.getByRole("radio", { name: "Editor" }).click();
  const windowRow = await editor.locator('[data-part="rules.condition"]').first().boundingBox();
  await window.locator('[data-part="window.close"]').click();
  await expect(window).toBeHidden();
  const settings = await openSettings(page, "Mail & junk");
  const rules = settings.locator('[data-part="rules"]');
  await rules.getByRole("button", { name: "Flag releases", exact: true }).click();
  const inSettings = rules.getByRole("form", { name: "Rule" });
  // The same compact row: the condition's controls side by side, as high as in the window.
  const row = inSettings.locator('[data-part="rules.condition"]').first();
  const box = await row.boundingBox();
  expect(box && windowRow && Math.abs(box.height - windowRow.height) <= 2).toBe(true);
  await expect(rulesFooter(inSettings).getByRole("button", { name: "Save rule" })).toBeVisible();
  await row.scrollIntoViewIfNeeded();
  // The footer stays in view while the page scrolls the form (design review, item 4).
  const footer = rulesFooter(inSettings);
  for (const name of ["Run", "Test rule", "Save rule"]) {
    const button = footer.getByRole("button", { name, exact: true });
    await expect(button).toBeInViewport();
    // On top: nothing of the form scrolls over it.
    expect(
      await button.evaluate((el) => {
        const r = el.getBoundingClientRect();
        return el.contains(document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2));
      }),
    ).toBe(true);
  }
  await page.screenshot({ path: screenshotFile("r2-rules", "settings-1024") });
});

for (const size of SIZES) {
  test(`a rule from a mail in a dark theme at ${String(size.width)}×${String(size.height)}`, async ({
    page,
  }) => {
    await openWorkInbox(page, server, size);
    const appearance = await openSettingsTab(page, "Appearance");
    await appearance
      .getByRole("radiogroup", { name: "Themes" })
      .getByRole("radio", { name: /Nocturne/ })
      .click();
    await showView(page, "Mail");
    await sectionInbox(accountSectionNamed(page, "Work")).click();
    const background = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    const editor = await ruleFromMessageRow(page, RELEASE);
    // The pickers, the values of this mail and the footer, in the dark theme.
    await expect(
      editor.getByRole("button", { name: "Values from this mail" }).first(),
    ).toBeVisible();
    await expect(rulesFooter(editor).getByRole("button", { name: "Save rule" })).toBeInViewport();
    expect(await background()).not.toBe("rgb(255, 255, 255)");
    await page.screenshot({
      path: screenshotFile("r2-rules", `dark-${String(size.width)}`),
    });
  });
}
