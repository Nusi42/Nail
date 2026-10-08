import { expect, test, type Locator, type Page } from "@playwright/test";
import { control, nailPerTest, screenshotFile } from "../support/nail";
import { rpcOk } from "../support/remote";
import {
  accountSection,
  readerAction,
  messageRows,
  syncAccount,
  openMail,
  DEMO_MULTI_CONVERSATIONS,
  WIDE_WINDOW,
  messageList,
  messageRow,
  applyLabel,
} from "../support/views";
import { DINNER, LAB, LAB_INBOX, LAB_SENT, LAB_TOTAL } from "../support/demo";

/**
 * P3 (ADR 0164): actions on a conversation row behave like Gmail, on the
 * real core with `--seed demo-multi`. "Lab move: timeline and packing" has
 * 12 messages in the work account: 9 in the Inbox and the user's 3 replies
 * in Sent.
 *
 * - Archive (e) moves the 9 Inbox messages to Archive; the replies stay in
 *   Sent; the next conversation is selected.
 * - Delete moves all 12 to Trash (the replies too); Undo brings them back.
 * - A card's own "More" menu deletes that one message only.
 * - Delete permanently from Trash asks with the message count.
 * - Moving the deleted conversation back to the Inbox returns the user's
 *   replies to Sent (P3 review).
 * - Several picked conversations act together; a Gmail conversation archives
 *   and takes labels (`l`) as one undoable change.
 *
 * Screenshots for the owner (light Frost, dark Graphite and Nocturne):
 * `<build dir>/screens/p3-actions/`.
 */

const server = nailPerTest({ seed: "demo-multi" });

// Each test starts demo-multi and waits for its first sync (up to 30 s on a
// loaded machine) before acting; the default 30 s would leave no time to act.
test.describe.configure({ timeout: 90_000 });

const toast = (page: Page) => page.locator('[data-part="mail-actions.toast"]');
const feed = (page: Page) => page.getByRole("feed");
const cards = (page: Page) => feed(page).locator('[data-part="thread-view.card"]');

interface FolderInfo {
  id: string;
  role: string;
  name: string;
}

/** Where the Lab move messages are: count per folder role of the work account. */
async function labFolders(): Promise<Record<string, number>> {
  const { folders } = (await rpcOk(
    server,
    { type: "ListFolders", account_id: "work" },
    "Folders",
  )) as unknown as { folders: FolderInfo[] };
  const out: Record<string, number> = {};
  for (const folder of folders) {
    const { messages } = (await rpcOk(
      server,
      { type: "ListMessages", folder_id: folder.id, offset: 0, limit: 1000 },
      "Messages",
    )) as unknown as { messages: { subject: string }[] };
    const count = messages.filter((m) => m.subject.includes("Lab move")).length;
    if (count > 0) out[folder.role === "other" ? folder.name : folder.role] = count;
  }
  return out;
}

const openApp = (page: Page, size = WIDE_WINDOW) =>
  openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS, size });

/** Selects a conversation row (the list keeps the focus for the keys). */
async function select(page: Page, subject: string): Promise<Locator> {
  const row = messageRow(page, subject);
  await expect(row).toHaveCount(1);
  await row.click();
  await expect(row).toHaveAttribute("aria-selected", "true");
  await expect(page.locator('[data-part="thread-view.subject"]')).toContainText(subject);
  return row;
}

const screenshotPath = (name: string): string => screenshotFile("p3-actions", name);

test("Archive (e) archives the conversation's Inbox messages, the replies stay in Sent", async ({
  page,
}) => {
  await openApp(page);
  const labRow = await select(page, LAB);
  const subjects = await messageRows(page).allInnerTexts();
  const index = subjects.findIndex((s) => s.includes("Lab move"));
  const nextText = (subjects[index + 1] ?? "").split("\n")[0] ?? "";
  await messageList(page).press("e");
  await expect(toast(page)).toContainText("Conversation archived");
  await expect(labRow).toHaveCount(0);
  await expect(messageRows(page)).toHaveCount(16);
  // Like Gmail, the next conversation is selected and open.
  await expect(messageRows(page).and(page.locator('[aria-selected="true"]'))).toContainText(
    nextText,
  );
  await expect.poll(labFolders, { timeout: 20_000 }).toEqual({
    archive: LAB_INBOX,
    sent: LAB_SENT,
  });
  await page.screenshot({ path: screenshotPath("archived-toast-frost") });
});

test("Delete moves the whole conversation to Trash, the replies too; Undo brings it back", async ({
  page,
}) => {
  await rpcOk(server, { type: "SetTheme", theme_id: "graphite" }, "Themes");
  await openApp(page);
  await select(page, LAB);
  await messageList(page).press("Delete");
  await expect(toast(page)).toContainText("Conversation moved to Trash");
  await expect(messageRows(page)).toHaveCount(16);
  await expect.poll(labFolders, { timeout: 20_000 }).toEqual({ trash: LAB_TOTAL });
  await page.screenshot({ path: screenshotPath("trash-toast-graphite") });

  // Undo: every message back where it was.
  await toast(page).getByRole("button", { name: "Undo" }).click();
  await expect(messageRow(page, LAB)).toHaveCount(1);
  await expect(messageRows(page)).toHaveCount(DEMO_MULTI_CONVERSATIONS);
  await expect.poll(labFolders, { timeout: 20_000 }).toEqual({
    inbox: LAB_INBOX,
    sent: LAB_SENT,
  });
  await expect(toast(page)).toContainText("Action undone");
});

test("a card's own Delete removes that one message only", async ({ page }) => {
  await rpcOk(server, { type: "SetTheme", theme_id: "nocturne" }, "Themes");
  await openApp(page);
  await select(page, LAB);
  // The newest card is open: its "More" menu acts on it alone.
  const card = cards(page).last();
  await expect(card).toHaveAttribute("data-state", /(^|\s)expanded(\s|$)/);
  await card.getByRole("button", { name: "More actions for this message" }).click();
  const menu = page.getByRole("menu");
  await expect(menu.getByRole("menuitem", { name: /^Mark this message unread/ })).toBeVisible();
  await page.screenshot({ path: screenshotPath("card-menu-nocturne") });
  await menu.getByRole("menuitem", { name: /^Delete this message/ }).click();
  await expect(toast(page)).toContainText("Message moved to Trash");
  await expect.poll(labFolders, { timeout: 20_000 }).toEqual({
    inbox: LAB_INBOX - 1,
    sent: LAB_SENT,
    trash: 1,
  });
  // The conversation stays listed, one message fewer.
  await expect(messageRow(page, LAB).locator('[data-part="message-list.row.count"]')).toHaveText(
    String(LAB_TOTAL - 1),
  );
});

/** Deletes the whole Lab move conversation (to Trash) and selects it there. */
async function selectLabInTrash(page: Page): Promise<void> {
  await openApp(page);
  await select(page, LAB);
  await messageList(page).press("Delete");
  await expect.poll(labFolders, { timeout: 20_000 }).toEqual({ trash: LAB_TOTAL });
  await accountSection(page, "work")
    .locator('[data-part="folder-tree.item"][data-role="trash"]')
    .first()
    .click();
  await select(page, LAB);
}

test("Delete permanently in Trash asks with the conversation's message count", async ({ page }) => {
  await selectLabInTrash(page);
  await messageList(page).press("Shift+Delete");
  const dialog = page.getByRole("dialog", { name: "Delete permanently?" });
  await expect(dialog).toContainText(
    `Delete this conversation's ${String(LAB_TOTAL)} messages permanently?`,
  );
  await dialog.getByRole("button", { name: "Delete permanently" }).click();
  await expect(dialog).toBeHidden();
  await expect(toast(page)).toContainText("Conversation deleted permanently");
  await expect.poll(labFolders, { timeout: 20_000 }).toEqual({});
});

test("a deleted conversation moved back to the Inbox returns the replies to Sent", async ({
  page,
}) => {
  await selectLabInTrash(page);
  await readerAction(page, "Move to…");
  const move = page.getByRole("dialog", { name: "Move to folder" });
  await expect(move).toBeVisible();
  await page.screenshot({ path: screenshotPath("restore-move-frost") });
  // The picker lists the account's folders (the work account's Trash is open).
  await move.locator('[data-part="folder-picker.button"][data-role="inbox"]').click();
  await expect(move).toBeHidden();
  // Like Gmail: the received mail goes to the Inbox, the user's own replies
  // back to Sent (not into the Inbox).
  await expect.poll(labFolders, { timeout: 20_000 }).toEqual({
    inbox: LAB_INBOX,
    sent: LAB_SENT,
  });
});

test("several picked conversations act together", async ({ page }) => {
  await openApp(page);
  await select(page, LAB);
  await messageRow(page, DINNER).click({ modifiers: ["ControlOrMeta"] });
  await expect(messageRow(page, LAB)).toHaveAttribute("aria-selected", "true");
  await expect(messageRow(page, DINNER)).toHaveAttribute("aria-selected", "true");
  await messageList(page).press("e");
  await expect(toast(page)).toContainText("2 conversations archived");
  await expect(messageRows(page)).toHaveCount(15);
});

const labelsOf = async (subject: string) =>
  (await control<{ id: string; labels: string[] }>(server, "gmail-message", { subject })).labels;

/** Switches the demo Gmail account on and shows its Inbox (four conversations). */
async function openGmailInbox(page: Page) {
  await page.setViewportSize({ width: 1600, height: 1000 });
  await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
  await syncAccount(page, "Gmail", 4);
}

test("labels (l) go on the whole Gmail conversation as one undoable change", async ({ page }) => {
  await openGmailInbox(page);
  await select(page, "Lunch on Friday?");
  await messageList(page).press("l");
  await applyLabel(page, "Receipts");
  await expect(toast(page)).toContainText("Label added to the conversation");
  await expect.poll(() => labelsOf("Lunch on Friday?"), { timeout: 20_000 }).toContain("Label_2");
  await toast(page).getByRole("button", { name: "Undo" }).click();
  await expect
    .poll(async () => (await labelsOf("Lunch on Friday?")).includes("Label_2"), {
      timeout: 20_000,
    })
    .toBe(false);
});

test("a Gmail conversation archives from the reader's toolbar", async ({ page }) => {
  await openGmailInbox(page);
  await select(page, "Autumn sale");
  await readerAction(page, "Archive");
  await expect(toast(page)).toContainText("Conversation archived");
  await expect(messageRows(page)).toHaveCount(3);
  await expect
    .poll(async () => (await labelsOf("Autumn sale")).includes("INBOX"), { timeout: 20_000 })
    .toBe(false);
});
