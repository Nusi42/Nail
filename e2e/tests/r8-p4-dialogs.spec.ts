import { expect, test, type Locator, type Page } from "@playwright/test";
import { draftInOutbox } from "../support/compose";
import { enableContoso, FIXTURE_NOW, graphMessageOf, graphSettingsOf } from "../support/exchange";
import { control, nailPerTest, serverMessages } from "../support/nail";
import { DEMO_MULTI_CONVERSATIONS, messageRows, openMail, windowById } from "../support/views";
import { expectDialogPattern } from "../support/windowTour";

/**
 * R8 (backlog "Dialogs without e2e", R6 follow-ups): the dialogs that had
 * unit tests only, end to end on `demo-multi` — the Exchange mailbox editor
 * and its remove question, the category delete question, the automatic
 * replies editor and the Focused Inbox classify question (mock Graph
 * tenant "Contoso"), and the Outbox's discard question for a draft. Each
 * follows the shared dialog pattern (ADR 0239) and its answer reaches the
 * server.
 */

const server = nailPerTest({ seed: "demo-multi" }, async (s) => {
  await control(s, "clock-set", { at: FIXTURE_NOW });
});

/** The viewer's system the dialog pattern is checked for (the page reports Windows). */
const OS = "windows";

/** Window `id` once it is visible, after checking the shared pattern. */
async function dialog(page: Page, id: string): Promise<Locator> {
  const window = windowById(page, id);
  await expect(window).toBeVisible();
  await expectDialogPattern(window, id, OS);
  return window;
}

/** The folder pane's "Other mailboxes" of Contoso. */
const mailboxes = (page: Page) => page.locator('[data-part="exchange-mailboxes"]');

test("the shared mailbox editor adds and renames a mailbox; Remove asks first", async ({
  page,
}) => {
  await page.goto(server.url);
  await enableContoso(page);
  await mailboxes(page).getByRole("button", { name: "Add shared mailbox…" }).click();
  const editor = await dialog(page, "exchange-mailbox-editor");
  await editor.getByRole("textbox", { name: "Mailbox address" }).fill("support@contoso.example");
  await editor.getByRole("button", { name: "Open mailbox" }).click();
  await expect(editor).toBeHidden();
  const tree = page.locator('[data-part="folder-tree"]');
  await expect(tree.locator('[data-part="exchange.mailbox-badge"]')).toBeVisible({
    timeout: 30_000,
  });

  // Edit: a name of the user's own.
  const item = mailboxes(page).locator('[data-part="exchange-mailboxes.item"]');
  await expect(item).toHaveCount(1);
  await item.locator('[data-part="exchange-mailboxes.edit"]').click();
  const edit = await dialog(page, "exchange-mailbox-editor");
  await expect(edit.getByRole("textbox", { name: "Mailbox address" })).toHaveValue(
    "support@contoso.example",
  );
  await edit.getByRole("textbox", { name: "Name", exact: true }).fill("Support desk");
  await edit.getByRole("button", { name: "Save", exact: true }).click();
  await expect(edit).toBeHidden();
  await expect(item.locator('[data-part="exchange-mailboxes.name"]')).toHaveText("Support desk");

  // Remove asks first: Cancel keeps the mailbox, Remove takes its folders away.
  await mailboxes(page).getByRole("button", { name: "Remove mailbox Support desk" }).click();
  const question = await dialog(page, "exchange-mailbox-remove");
  await expect(question).toContainText("Remove the mailbox “Support desk”?");
  await expect(question.getByRole("button", { name: "Cancel" })).toBeFocused();
  await question.getByRole("button", { name: "Cancel" }).click();
  await expect(question).toBeHidden();
  await expect(item).toHaveCount(1);
  await mailboxes(page).getByRole("button", { name: "Remove mailbox Support desk" }).click();
  await windowById(page, "exchange-mailbox-remove")
    .getByRole("button", { name: "Remove", exact: true })
    .click();
  await expect(item).toHaveCount(0);
  await expect(tree.locator('[data-part="exchange.mailbox-badge"]')).toHaveCount(0);
});

test("Exchange: a category is deleted after its question, automatic replies are saved, a message is classified", async ({
  page,
}) => {
  await page.goto(server.url);
  await enableContoso(page);

  // Category delete: the question names it; Delete removes it on the server.
  const categories = page.locator('[data-part="exchange-categories"]');
  await categories.getByRole("button", { name: "Delete category Red category" }).click();
  const question = await dialog(page, "exchange-category-delete");
  await expect(question).toContainText("Delete the category “Red category”?");
  await question.getByRole("button", { name: "Delete", exact: true }).click();
  await expect(question).toBeHidden();
  await expect
    .poll(async () => (await graphSettingsOf(server)).categories.map((c) => c.displayName))
    .not.toContain("Red category");
  await expect(
    categories.getByRole("button", { name: "Delete category Red category" }),
  ).toHaveCount(0);

  // Automatic replies: the editor from the banner saves a new text.
  await page
    .locator('[data-part="automatic-replies-banner"]')
    .getByRole("button", { name: /^Edit/ })
    .click();
  const replies = await dialog(page, "exchange-automatic-replies");
  await replies
    .getByRole("textbox", { name: /in my organization/ })
    .fill("Away until the conference ends.");
  await replies.getByRole("button", { name: "Save", exact: true }).click();
  await expect(replies).toBeHidden();
  await expect
    .poll(async () =>
      String((await graphSettingsOf(server)).automatic_replies.internalReplyMessage),
    )
    .toContain("Away until the conference ends.");

  // Classify: "Move to Focused / Other…" asks, offers "always", and moves
  // it on the server.
  await messageRows(page).filter({ hasText: "Engine review on Monday" }).click({ button: "right" });
  await page.getByRole("menu").getByRole("menuitem", { name: "Move to Focused / Other…" }).click();
  const classify = await dialog(page, "exchange-classify");
  await expect(classify).toContainText("Move this message to Other?");
  await classify.getByRole("checkbox", { name: /^Always move messages from / }).check();
  await classify.getByRole("button", { name: "Move to Other", exact: true }).click();
  await expect(classify).toBeHidden();
  await expect
    .poll(async () => (await graphMessageOf(server, "Engine review on Monday"))?.inference)
    .toBe("other");
});

test("the Outbox asks before a draft is discarded, and Discard removes it everywhere", async ({
  page,
}) => {
  await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
  const item = await draftInOutbox(page, server, "Draft to throw away", "Not needed after all");
  await item.getByRole("button", { name: "Discard", exact: true }).click();
  const question = item.locator('[data-part="dialog.confirm"]');
  await expect(question).toContainText("Discard the draft “Draft to throw away”?");
  // The destructive verb is never the default: the focus starts on Cancel.
  await expect(question.getByRole("button", { name: "Cancel" })).toBeFocused();
  await question.getByRole("button", { name: "Cancel" }).click();
  await expect(item.getByRole("button", { name: "Continue writing" })).toBeVisible();

  await item.getByRole("button", { name: "Discard", exact: true }).click();
  await item.getByRole("button", { name: "Discard draft" }).click();
  await expect(item).toHaveCount(0);
  await expect
    .poll(async () => (await serverMessages(server, "Drafts")).map((m) => m.subject))
    .not.toContain("Draft to throw away");
});
