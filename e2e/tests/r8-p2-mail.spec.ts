import { expect, test, type Locator, type Page } from "@playwright/test";
import { nailPerTest, serverMessages } from "../support/nail";
import {
  WORK_INBOX_ROWS,
  newRuleFromText,
  openRulesWindow,
  openWorkInbox,
  rulesToast,
  saveRule,
} from "../support/rules";
import {
  messageList,
  messageRows,
  openMail,
  rail,
  readerToolbar,
  showView,
  viewRegion,
  windowById,
} from "../support/views";

/**
 * R8 (mail and windows UI) against `nail serve --seed demo` / `demo-multi`:
 * favorites as views (Snoozed is selected while its window is the active
 * view; Flagged leaves out Trash), the folder a search runs in marked in
 * the folder tree, the rail's account menu, the More menu anchored at its
 * button's end, and "Run rule" on a whole list after select all (undone in
 * one step).
 */

const server = nailPerTest({ seed: "demo" });

const favorite = (page: Page, name: string) =>
  page.locator(`[data-part="folder-tree.favorite"][data-favorite="${name}"]`);

/** The subject a row shows. */
async function subjectOf(row: Locator): Promise<string> {
  const text = await row.locator('[data-part="message-list.row.subject"]').first().textContent();
  return (text ?? "").trim();
}

// The Work account of demo-multi has a Trash (the demo account has none, so
// Delete fails there and the row comes back).
test.describe("favorites as views", () => {
  const multi = nailPerTest({ seed: "demo-multi" });

  test("favorites act as views: Snoozed while its window is in front; Flagged leaves out Trash", async ({
    page,
  }) => {
    await openWorkInbox(page, multi);
    // Flag the first conversation, then move it to Trash.
    const row = messageRows(page).first();
    const subject = await subjectOf(row);
    await row.click();
    await messageList(page).focus();
    await page.keyboard.press("s");
    await favorite(page, "flagged").click();
    await expect(favorite(page, "flagged")).toHaveAttribute("aria-selected", "true");
    const flaggedRow = messageRows(page).filter({ hasText: subject });
    await expect(flaggedRow.first()).toBeVisible();
    await flaggedRow.first().click();
    await page.keyboard.press("Delete");
    // In Trash it no longer counts as flagged mail to see to.
    await expect(flaggedRow).toHaveCount(0);
    await favorite(page, "all-inboxes").click();
    await favorite(page, "flagged").click();
    await expect(messageRows(page).first()).toBeVisible();
    await expect(messageRows(page).filter({ hasText: subject })).toHaveCount(0);

    // Snoozed opens its window and is the selected favorite while it is in front.
    await favorite(page, "snoozed").click();
    const snoozed = windowById(page, "snoozed");
    await expect(snoozed).toBeVisible();
    await expect(favorite(page, "snoozed")).toHaveAttribute("aria-selected", "true");
    await expect(favorite(page, "flagged")).toHaveAttribute("aria-selected", "false");
    await expect(page.locator('[data-part^="folder-tree."][aria-selected="true"]')).toHaveCount(1);
    await snoozed.getByRole("button", { name: "Close" }).first().click();
    await expect(snoozed).toBeHidden();
    await expect(favorite(page, "flagged")).toHaveAttribute("aria-selected", "true");
    await expect(favorite(page, "snoozed")).toHaveAttribute("aria-selected", "false");
  });
});

test("the folder tree marks the folder a search runs in", async ({ page }) => {
  await openMail(page, server.url);
  const searched = page.locator('[data-part="folder-tree.item"][data-state="searched"]');
  const search = page.getByRole("combobox", { name: "Search mail" });
  await search.fill("a");
  await search.press("Enter");
  await expect(searched).toHaveCount(0);
  // A folder change runs the search in that folder (R2): the tree marks it.
  const sent = page.locator('[data-part="folder-tree.item"][data-role="sent"]').first();
  await sent.click();
  await expect(sent).toHaveAttribute("data-state", "searched");
  await expect(sent).toHaveAccessibleDescription("The search runs in this folder");
  await expect(searched).toHaveCount(1);
  // Clearing the search clears the mark.
  await search.fill("");
  await search.press("Enter");
  await expect(searched).toHaveCount(0);
});

test("the rail's avatar opens the account menu and shows the chosen account", async ({ page }) => {
  await openMail(page, server.url);
  await showView(page, "Calendar");
  const avatar = rail(page).getByRole("button", { name: /^Account: / });
  await avatar.click();
  const menu = page.getByRole("menu", { name: "Accounts" });
  await expect(menu).toBeVisible();
  await expect(avatar).toHaveAttribute("aria-expanded", "true");
  await expect(menu.getByRole("menuitem", { name: /^Add account/ })).toBeVisible();
  // A second click closes it.
  await avatar.click();
  await expect(menu).toBeHidden();
  // The keyboard: Enter opens it on its first item, Escape gives the focus back.
  await avatar.focus();
  await page.keyboard.press("Enter");
  // The accounts are choices; the one the avatar names is checked.
  const shown = menu.getByRole("menuitemradio", { checked: true });
  await expect(menu.getByRole("menuitemradio").first()).toBeFocused();
  await expect(avatar).toHaveAccessibleName(`Account: ${(await shown.textContent()) ?? ""}`);
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await expect(avatar).toBeFocused();
  await page.keyboard.press("ArrowRight");
  await expect(menu).toBeVisible();
  await menu.getByRole("menuitemradio", { name: "All accounts" }).click();
  await expect(viewRegion(page, "Mail")).toBeVisible();
  await expect(favorite(page, "all-inboxes")).toHaveAttribute("aria-selected", "true");
});

test("More opens under its button, its right edge at the button's", async ({ page }) => {
  await openMail(page, server.url);
  await messageRows(page).first().click();
  const more = readerToolbar(page).getByRole("button", { name: "More", exact: true });
  await more.click();
  const menu = page.getByRole("menu", { name: "More" });
  await expect(menu).toBeVisible();
  const button = await more.boundingBox();
  const box = await menu.boundingBox();
  expect(button).not.toBeNull();
  expect(box).not.toBeNull();
  if (button && box) {
    expect(Math.abs(box.x + box.width - (button.x + button.width))).toBeLessThanOrEqual(1);
    expect(box.y).toBeGreaterThanOrEqual(button.y + button.height - 1);
  }
});

/** A manual rule that flags every message it runs on. */
const FLAG_ALL = `rule "Flag all" trigger="manual" {
    then {
        flag
    }
}
`;

test.describe("rules on a whole list", () => {
  const multi = nailPerTest({ seed: "demo-multi" });

  test("Run rule after select all runs on every row and is undone in one step", async ({
    page,
  }) => {
    await openWorkInbox(page, multi);
    const window = await openRulesWindow(page);
    await saveRule(await newRuleFromText(window, FLAG_ALL));
    await window.getByRole("button", { name: "Close" }).first().click();
    await expect(window).toBeHidden();

    const flaggedCount = async () =>
      (await serverMessages(multi, "INBOX")).filter((m) => m.flags.includes("\\Flagged")).length;
    const before = await flaggedCount();
    // The list shows conversations: a run on every row reaches every
    // message of every listed conversation.
    const messages = (await serverMessages(multi, "INBOX")).length;
    expect(messages).toBeGreaterThan(WORK_INBOX_ROWS);
    await messageRows(page).first().click();
    await messageList(page).focus();
    await page.keyboard.press("ControlOrMeta+a");
    await expect(page.locator('[data-part="message-list.selection-summary"]')).toHaveText(
      `All ${String(WORK_INBOX_ROWS)} selected`,
    );
    await messageRows(page).nth(3).click({ button: "right" });
    await page.getByRole("menu").getByRole("menuitem", { name: "Run rule" }).click();
    await page
      .getByRole("menu", { name: "Run rule" })
      .getByRole("menuitem", { name: "Flag all" })
      .click();
    const toast = rulesToast(page);
    await expect(toast).toContainText("The rule ran");
    await expect.poll(flaggedCount, { timeout: 20_000 }).toBe(messages);
    await toast.getByRole("button", { name: "Undo" }).click();
    await expect(toast).toContainText("Everything the run did was undone.");
    await expect.poll(flaggedCount, { timeout: 20_000 }).toBe(before);
  });
});
