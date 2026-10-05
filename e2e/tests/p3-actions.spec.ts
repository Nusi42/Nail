import { mkdirSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { buildOutputDir, control, startNail, type NailServer } from "../support/nail";
import { rpcOk } from "../support/remote";
import {
  accountSection,
  accountSectionNamed,
  openSettingsTab,
  readerAction,
  sectionInbox,
  showView,
  viewRegion,
} from "../support/views";

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
 * - Several picked conversations act together; a Gmail conversation archives.
 *
 * Screenshots for the owner (light Frost, dark Graphite and Nocturne):
 * `<build dir>/screens/p3-actions/`.
 */

const LAB = "Lab move: timeline and packing";
const DINNER = "Dinner on Saturday?";
const LAB_INBOX = 9;
const LAB_SENT = 3;
const LAB_TOTAL = LAB_INBOX + LAB_SENT;

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo-multi" });
});

test.afterEach(async () => {
  // `beforeEach` may have failed before assigning it.
  await (server as NailServer | undefined)?.stop();
});

const rows = (page: Page) => page.locator('[data-part="message-list.row"]');
const rowOf = (page: Page, subject: string) => rows(page).filter({ hasText: subject });
const toast = (page: Page) => page.locator('[data-part="mail-actions.toast"]');
const feed = (page: Page) => page.getByRole("feed");
const cards = (page: Page) => feed(page).locator('[data-part="thread-view.card"]');
const list = (page: Page) => page.locator('[data-part="message-list"]');

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

async function openApp(page: Page, size = { width: 1600, height: 1000 }) {
  await page.setViewportSize(size);
  await page.goto(server.url);
  // 17 conversations in the unified inbox (P2's demo threads included).
  await expect(rows(page)).toHaveCount(17, { timeout: 30_000 });
}

/** Selects a conversation row (the list keeps the focus for the keys). */
async function select(page: Page, subject: string): Promise<Locator> {
  const row = rowOf(page, subject);
  await expect(row).toHaveCount(1);
  await row.click();
  await expect(row).toHaveAttribute("aria-selected", "true");
  await expect(page.locator('[data-part="thread-view.subject"]')).toContainText(subject);
  return row;
}

function screenshotPath(name: string): string {
  const dir = buildOutputDir(path.join("screens", "p3-actions"));
  mkdirSync(dir, { recursive: true });
  return path.join(dir, `${name}.png`);
}

test("Archive (e) archives the conversation's Inbox messages, the replies stay in Sent", async ({
  page,
}) => {
  await openApp(page);
  const labRow = await select(page, LAB);
  const subjects = await rows(page).allInnerTexts();
  const index = subjects.findIndex((s) => s.includes("Lab move"));
  const nextText = (subjects[index + 1] ?? "").split("\n")[0] ?? "";
  await list(page).press("e");
  await expect(toast(page)).toContainText("Conversation archived");
  await expect(labRow).toHaveCount(0);
  await expect(rows(page)).toHaveCount(16);
  // Like Gmail, the next conversation is selected and open.
  await expect(rows(page).and(page.locator('[aria-selected="true"]'))).toContainText(nextText);
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
  await list(page).press("Delete");
  await expect(toast(page)).toContainText("Conversation moved to Trash");
  await expect(rows(page)).toHaveCount(16);
  await expect.poll(labFolders, { timeout: 20_000 }).toEqual({ trash: LAB_TOTAL });
  await page.screenshot({ path: screenshotPath("trash-toast-graphite") });

  // Undo: every message back where it was.
  await toast(page).getByRole("button", { name: "Undo" }).click();
  await expect(rowOf(page, LAB)).toHaveCount(1);
  await expect(rows(page)).toHaveCount(17);
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
  await expect(rowOf(page, LAB).locator('[data-part="message-list.row.count"]')).toHaveText(
    String(LAB_TOTAL - 1),
  );
});

test("Delete permanently in Trash asks with the conversation's message count", async ({ page }) => {
  await openApp(page);
  await select(page, LAB);
  await list(page).press("Delete");
  await expect.poll(labFolders, { timeout: 20_000 }).toEqual({ trash: LAB_TOTAL });
  await accountSection(page, "work")
    .locator('[data-part="folder-tree.item"][data-role="trash"]')
    .first()
    .click();
  await select(page, LAB);
  await list(page).press("Shift+Delete");
  const dialog = page.getByRole("dialog", { name: "Delete permanently?" });
  await expect(dialog).toContainText(
    `Delete this conversation's ${String(LAB_TOTAL)} messages permanently?`,
  );
  await dialog.getByRole("button", { name: "Delete permanently" }).click();
  await expect(dialog).toBeHidden();
  await expect(toast(page)).toContainText("Conversation deleted permanently");
  await expect.poll(labFolders, { timeout: 20_000 }).toEqual({});
});

test("several picked conversations act together", async ({ page }) => {
  await openApp(page);
  await select(page, LAB);
  await rowOf(page, DINNER).click({ modifiers: ["ControlOrMeta"] });
  await expect(rowOf(page, LAB)).toHaveAttribute("aria-selected", "true");
  await expect(rowOf(page, DINNER)).toHaveAttribute("aria-selected", "true");
  await list(page).press("e");
  await expect(toast(page)).toContainText("2 conversations archived");
  await expect(rows(page)).toHaveCount(15);
});

test("a Gmail conversation archives from the reader's toolbar", async ({ page }) => {
  const labelsOf = async (subject: string) =>
    (await control<{ id: string; labels: string[] }>(server, "gmail-message", { subject })).labels;
  await page.setViewportSize({ width: 1600, height: 1000 });
  await page.goto(server.url);
  await expect(rows(page)).toHaveCount(17, { timeout: 30_000 });
  // Settings → Accounts → Gmail → "Synchronize this account".
  await showView(page, "Settings");
  const settings = viewRegion(page, "Settings");
  await openSettingsTab(page, "Accounts");
  await settings.getByRole("button", { name: /^Gmail/ }).click();
  await settings.getByRole("checkbox", { name: "Synchronize this account" }).check();
  await settings.getByRole("button", { name: "Save", exact: true }).click();
  await expect(settings.getByText("Settings saved.")).toBeVisible();
  await showView(page, "Mail");
  const gmail = accountSectionNamed(page, "Gmail");
  await expect(gmail).toHaveAttribute("data-status", "online", { timeout: 30_000 });
  await sectionInbox(gmail).click();
  await expect(rows(page)).toHaveCount(4, { timeout: 30_000 });
  await select(page, "Autumn sale");
  await readerAction(page, "Archive");
  await expect(toast(page)).toContainText("Conversation archived");
  await expect(rows(page)).toHaveCount(3);
  await expect
    .poll(async () => (await labelsOf("Autumn sale")).includes("INBOX"), { timeout: 20_000 })
    .toBe(false);
});
