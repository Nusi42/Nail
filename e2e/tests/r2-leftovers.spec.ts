import { writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { nailPerTest, screenshotFile } from "../support/nail";
import {
  accountSectionNamed,
  DEMO_MULTI_CONVERSATIONS,
  messageList,
  messageRow,
  messageRows,
  openMail,
  sectionInbox,
} from "../support/views";
import {
  flaggedOnServer,
  openWorkInbox,
  RELEASE,
  rulesToast,
  WELCOME,
  WORK_INBOX_ROWS,
} from "../support/rules";

/**
 * R2 (ADR 0222), the R1 leftovers on the real core (`demo-multi`): "Run
 * rule" is a hover / arrow-key submenu of the saved rules, a rule named
 * "@home" shows as written; the search hints are a combobox popup over the
 * list (it never moves the list, never opens on focus alone, and closes
 * with Escape and a click outside); a folder change searches the typed
 * text in the new folder.
 */

const server = nailPerTest({ seed: "demo-multi" });

const AT_RULE = "@home";

/** Two manual rules that flag messages, written as the user's `rules.kdl`. */
const RULES = `rule "${AT_RULE}" trigger="manual" {
    then {
        flag
    }
}

rule "Flag releases" trigger="manual" {
    when {
        subject contains="Release notes"
    }
    then {
        flag
    }
}
`;

const searchBox = (page: Page) => page.getByRole("combobox", { name: "Search mail" });
const hints = (page: Page) => page.getByRole("listbox", { name: "Search fields" });
const workSection = (page: Page) => accountSectionNamed(page, "Work");

const flagged = (subject: string) => flaggedOnServer(server, subject);

test("Run rule opens on hover, lists the rules as written and runs one", async ({ page }) => {
  // The core watches `rules.kdl` for hand edits.
  writeFileSync(path.join(server.configDir, "rules.kdl"), RULES);
  await openWorkInbox(page, server);

  const runRule = page.getByRole("menu", { name: "Message actions" }).getByRole("menuitem", {
    name: "Run rule",
  });
  const rulesMenu = page.getByRole("menu", { name: "Run rule" });
  // The rules show up once the core read the file (the menu loads them when it opens).
  await expect(async () => {
    await page.keyboard.press("Escape");
    await messageRow(page, WELCOME).click({ button: "right" });
    await expect(runRule).toHaveAttribute("aria-haspopup", "menu");
    await runRule.hover();
    await expect(rulesMenu.getByRole("menuitem", { name: AT_RULE })).toBeVisible({
      timeout: 1_000,
    });
  }).toPass({ timeout: 20_000 });

  // A submenu beside the menu, not a menu swapped in its place.
  await expect(page.getByRole("menu", { name: "Message actions" })).toBeVisible();
  await expect(runRule).toHaveAttribute("aria-expanded", "true");
  await expect(rulesMenu.getByRole("menuitem")).toHaveText([
    "All rules that are on",
    AT_RULE,
    "Flag releases",
  ]);
  await page.screenshot({ path: screenshotFile("r2-leftovers", "run-rule-submenu") });

  // The rule runs on the row (its toast offers Undo, as in R1).
  await rulesMenu.getByRole("menuitem", { name: AT_RULE }).click();
  await expect(rulesToast(page).getByRole("button", { name: "Undo" })).toBeVisible();
  await expect.poll(() => flagged(WELCOME)).toBe(true);
  expect(await flagged(RELEASE)).toBe(false);

  // The keyboard: Shift+F10 on the folder, ArrowRight into the rules, Enter runs.
  await sectionInbox(workSection(page)).focus();
  await page.keyboard.press("Shift+F10");
  const folderMenu = page.getByRole("menu", { name: "Folder actions" });
  const folderRun = folderMenu.getByRole("menuitem", { name: "Run rule" });
  await folderRun.focus();
  await page.keyboard.press("ArrowRight");
  await expect(rulesMenu.getByRole("menuitem").first()).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await expect(rulesMenu).toHaveCount(0);
  await expect(folderRun).toBeFocused();
  await page.keyboard.press("ArrowRight");
  await rulesMenu.getByRole("menuitem", { name: "Flag releases" }).focus();
  await page.keyboard.press("Enter");
  await expect.poll(() => flagged(RELEASE)).toBe(true);
});

test("the search hints float over the list and let clicks reach it", async ({ page }) => {
  await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
  const list = messageList(page);
  const before = await list.boundingBox();
  const input = searchBox(page);

  // Focus alone opens nothing: a right-click on a row right after reaches it.
  await input.click();
  await expect(hints(page)).toHaveCount(0);
  await messageRows(page).first().click({ button: "right" });
  await expect(page.getByRole("menu", { name: "Message actions" })).toBeVisible();
  await page.keyboard.press("Escape");

  // Typing a field name opens the popup over the list, which does not move.
  await input.click();
  await input.pressSequentially("su");
  await expect(hints(page)).toBeVisible();
  await expect(input).toHaveAttribute("aria-expanded", "true");
  expect(await list.boundingBox()).toEqual(before);
  await page.screenshot({ path: screenshotFile("r2-leftovers", "search-hints") });

  // Escape closes only the popup.
  await input.press("Escape");
  await expect(hints(page)).toHaveCount(0);
  await expect(input).toHaveValue("su");

  // Down picks a hint, Enter takes it.
  await input.press("ArrowDown");
  await expect(hints(page).getByRole("option").first()).toHaveAttribute("aria-selected", "true");
  await input.press("Enter");
  await expect(input).toHaveValue("subject:");
  await expect(hints(page)).toHaveCount(0);

  // After a space, ArrowDown offers every field; a click outside closes them.
  await input.press("End");
  await input.pressSequentially("x ");
  await expect(hints(page)).toHaveCount(0);
  await input.press("ArrowDown");
  await expect(hints(page)).toBeVisible();
  await page.locator('[data-part="folder-tree"]').click({ position: { x: 4, y: 4 } });
  await expect(hints(page)).toHaveCount(0);
});

test("the search field offers a field's values and puts the picked one in", async ({ page }) => {
  await openWorkInbox(page, server);
  const input = searchBox(page);
  await input.click();
  await input.pressSequentially("folder:Arch");
  const values = page.getByRole("group", { name: "Values for this search field" });
  await expect(values).toBeVisible();
  // One combobox: it announces the popup, and the arrow keys reach the values.
  await expect(input).toHaveAttribute("aria-expanded", "true");
  const archive = values.getByRole("option", { name: /^Archive/ });
  await input.press("ArrowDown");
  await expect(input).toHaveAttribute(
    "aria-activedescendant",
    (await archive.getAttribute("id")) ?? "",
  );
  await archive.click();
  await expect(input).toHaveValue("folder:Archive ");
  await expect(input).toBeFocused();
  await expect(values).toHaveCount(0);
});

test("a folder change searches the typed text in the new folder", async ({ page }) => {
  await openWorkInbox(page, server);
  const input = searchBox(page);
  await input.fill("Release notes");
  await input.press("Enter");
  await expect(messageList(page)).toHaveAttribute("data-state", "search");
  await expect(messageRow(page, RELEASE)).toBeVisible();

  // Another folder of the account: the same text, searched there.
  const other = workSection(page)
    .locator('[data-part="folder-tree.item"][data-depth="0"]:not([data-role="inbox"])')
    .first();
  await other.click();
  await expect(input).toHaveValue("Release notes");
  await expect(messageList(page)).toHaveAttribute("data-state", "search");
  await expect(messageRow(page, RELEASE)).toHaveCount(0);

  // Back in the inbox the text finds the message again.
  await sectionInbox(workSection(page)).click();
  await expect(messageList(page)).toHaveAttribute("data-state", "search");
  await expect(messageRow(page, RELEASE)).toBeVisible();
  await expect(input).toHaveValue("Release notes");

  // Clear ends the search: the folder's messages.
  await page.getByRole("button", { name: "Clear search" }).click();
  await expect(messageList(page)).not.toHaveAttribute("data-state", "search");
  await expect(messageRows(page)).toHaveCount(WORK_INBOX_ROWS);
});
