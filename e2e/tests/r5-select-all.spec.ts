import { writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { control, nailPerTest, serverMessages, type NailServer } from "../support/nail";
import {
  accountSectionNamed,
  messageList,
  messageRow,
  messageRows,
  moveSelectionTo,
  opsItem,
  sectionInbox,
} from "../support/views";

/**
 * Select all (R5, ADR 0235): Ctrl+A in the message list selects the whole
 * list, rows not loaded yet included; Ctrl + click leaves a row out; the
 * list says "All N selected"; an action asks once with the true count and
 * runs as one change; Esc clears. Web mode against `--seed demo-multi`.
 */

/**
 * The select-all keys as a layout binds them (`list.select-all`,
 * `list.clear-selection` in the list context; Cmd+A on macOS).
 */
const SELECT_KEYS = [
  'layout "E2E select all" extends="default" {',
  '    keys context="list" {',
  '        bind "ctrl+a" "list.select-all"',
  '        bind "meta+a" "list.select-all"',
  '        bind "escape" "list.clear-selection"',
  "    }",
  "}",
  "",
].join("\n");

const server = nailPerTest({ seed: "demo-multi" }, (started) => {
  writeFileSync(path.join(started.configDir, "layout.kdl"), SELECT_KEYS);
});

test.describe.configure({ timeout: 240_000 });

/** More conversations than the list loads in one page (100 rows). */
const BULK = 130;
const PREFIX = "R5 select";
const subjectOf = (n: number) => `${PREFIX} ${String(n).padStart(3, "0")}`;
const TOP_ROW = subjectOf(BULK);
const BOTTOM_ROW = subjectOf(1);

/** Delivers `count` messages to the Work inbox, each its own conversation, newer ones later. */
async function deliver(target: NailServer, count: number): Promise<void> {
  for (let n = 1; n <= count; n++) {
    const date = new Date(Date.UTC(2029, 0, 1, 0, n)).toUTCString();
    await control(target, "imap-deliver", {
      raw: `From: Bulk Sender <bulk@example.net>\r\nTo: ada@work.example\r\nSubject: ${subjectOf(n)}\r\nDate: ${date}\r\nMessage-ID: <r5-select-${String(n)}@example.net>\r\n\r\nSelect all ${String(n)}\r\n`,
    });
  }
}

/** Opens the Work inbox once `count` delivered messages are listed. */
async function openWorkInbox(page: Page, count: number): Promise<void> {
  await page.goto(server.url);
  await sectionInbox(accountSectionNamed(page, "Work")).click();
  await deliver(server, count);
  await expect(messageRow(page, subjectOf(count))).toHaveCount(1, { timeout: 60_000 });
}

const summary = (page: Page) => page.locator('[data-part="message-list.selection-summary"]');
const bulkQuestion = (page: Page) =>
  page.locator('[data-part="mail-confirm"][data-confirm="bulk"]');
const selectedRows = (page: Page) => messageRows(page).and(page.locator('[aria-selected="true"]'));

/** The number "All N selected" shows. */
async function selectedCount(page: Page): Promise<number> {
  const text = (await summary(page).textContent()) ?? "";
  return Number.parseInt(text.replace(/\D/g, ""), 10);
}

/** Clicks a row, then Ctrl+A in the list: the whole list is selected. */
async function selectAll(page: Page, row: Locator): Promise<void> {
  await row.click();
  await messageList(page).focus();
  await page.keyboard.press("ControlOrMeta+a");
  await expect(summary(page)).toHaveText(/^All \d+ selected$/);
}

/** Scrolls the list down until `subject`'s row is loaded and shown. */
async function scrollTo(page: Page, subject: string): Promise<void> {
  const list = messageList(page);
  await expect
    .poll(
      async () => {
        await list.evaluate((el) => {
          el.scrollTop = el.scrollHeight;
        });
        return messageRow(page, subject).count();
      },
      { timeout: 60_000 },
    )
    .toBe(1);
}

/** The delivered messages in a server mailbox. */
async function onServer(mailbox: string): Promise<number> {
  const messages = await serverMessages(server, mailbox);
  return messages.filter((m) => m.subject.startsWith(PREFIX)).length;
}

test("Ctrl+A selects a list larger than one page; deselect one, move the rest", async ({
  page,
}) => {
  await openWorkInbox(page, BULK);
  await selectAll(page, messageRow(page, TOP_ROW));
  const all = await selectedCount(page);
  expect(all).toBeGreaterThan(BULK);
  await expect(messageList(page)).toHaveAttribute("aria-multiselectable", "true");

  // Ctrl + click leaves a row out; the true count follows.
  await messageRow(page, TOP_ROW).click({ modifiers: ["ControlOrMeta"] });
  await expect(messageRow(page, TOP_ROW)).toHaveAttribute("aria-selected", "false");
  await expect(summary(page)).toHaveText(`All ${String(all - 1)} selected`);

  // Rows not loaded at Ctrl+A come selected when they scroll in.
  await scrollTo(page, BOTTOM_ROW);
  await expect(messageRow(page, BOTTOM_ROW)).toHaveAttribute("aria-selected", "true");

  // Move: one question with the true count, then one change.
  await moveSelectionTo(page, "Archive");
  const question = bulkQuestion(page);
  await expect(question).toContainText(
    `This affects all ${String(all - 1)} selected conversations`,
  );
  await question.getByRole("button", { name: "Move all" }).click();
  await expect(question).toBeHidden();

  // Everything but the row left out reaches the server's Archive; the
  // status bar's "Saving N changes" ends idle.
  await expect.poll(() => onServer("Archive"), { timeout: 120_000 }).toBe(BULK - 1);
  expect(await onServer("INBOX")).toBe(1);
  await expect(opsItem(page)).toHaveCount(0, { timeout: 30_000 });
  await expect(messageRows(page)).toHaveCount(1);
  await expect(messageRow(page, TOP_ROW)).toHaveCount(1);
  await expect(summary(page)).toHaveCount(0);

  // One undo brings every message back.
  await page.getByRole("button", { name: "Undo" }).click();
  await expect.poll(() => onServer("INBOX"), { timeout: 120_000 }).toBe(BULK);
  await expect(opsItem(page)).toHaveCount(0, { timeout: 30_000 });
});

test("Ctrl+A in a search selects every hit", async ({ page }) => {
  const HITS = 12;
  await openWorkInbox(page, HITS);
  const search = page.getByRole("combobox", { name: "Search mail" });
  await search.fill(`"${PREFIX}"`);
  await search.press("Enter");
  await expect(messageList(page)).toHaveAttribute("data-state", "search");
  await expect(messageRows(page)).toHaveCount(HITS);

  await selectAll(page, messageRow(page, subjectOf(HITS)));
  await expect(summary(page)).toHaveText(`All ${String(HITS)} selected`);
  await expect(selectedRows(page)).toHaveCount(HITS);

  // Flag (s): asks once for the whole search, then flags every hit.
  await messageList(page).focus();
  await page.keyboard.press("s");
  const question = bulkQuestion(page);
  await expect(question).toContainText(String(HITS));
  await question.getByRole("button", { name: "Flag all" }).click();
  await expect
    .poll(
      async () =>
        (await serverMessages(server, "INBOX")).filter(
          (m) => m.subject.startsWith(PREFIX) && m.flags.includes("\\Flagged"),
        ).length,
      { timeout: 60_000 },
    )
    .toBe(HITS);
  await expect(opsItem(page)).toHaveCount(0, { timeout: 30_000 });
  // The hits stay selected (flags do not take rows out of the list).
  await expect(summary(page)).toHaveText(`All ${String(HITS)} selected`);
});

test("Esc clears select all; Cancel acts on nothing", async ({ page }) => {
  const COUNT = 5;
  await openWorkInbox(page, COUNT);
  await selectAll(page, messageRow(page, subjectOf(COUNT)));
  expect(await selectedCount(page)).toBeGreaterThan(COUNT);

  // Archive (e), then Cancel: nothing moves, the selection stays.
  await messageList(page).focus();
  await page.keyboard.press("e");
  const question = bulkQuestion(page);
  await expect(question.getByRole("button", { name: "Cancel" })).toBeFocused();
  await question.getByRole("button", { name: "Cancel" }).click();
  await expect(question).toBeHidden();
  expect(await onServer("INBOX")).toBe(COUNT);
  await expect(summary(page)).toBeVisible();

  await messageList(page).focus();
  await page.keyboard.press("Escape");
  await expect(summary(page)).toHaveCount(0);
  await expect(selectedRows(page)).toHaveCount(1);
});
