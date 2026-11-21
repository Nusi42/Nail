import { expect, test, type Page } from "@playwright/test";
import { control, nailPerTest, serverMessages } from "../support/nail";
import {
  composerWindow,
  DEMO_MULTI_CONVERSATIONS,
  messageList,
  messageRow,
  openMail,
  readerAction,
  refreshList,
  WIDE_WINDOW,
} from "../support/views";
import { LAB, LAB_INBOX } from "../support/demo";

/**
 * R4 (ADR 0230) on the real core with `--seed demo-multi`:
 *
 * - The move window never moves to a partial match on Enter: the arrow
 *   keys highlight an option (visibly), Enter takes the highlighted one,
 *   all from the keyboard.
 * - Moving into Drafts asks first, with the message count; Cancel moves
 *   nothing.
 * - A received message in Drafts (no `\Draft` flag) opens in the reader,
 *   not in the composer, and "Move out of Drafts" takes it to the Inbox.
 */

const server = nailPerTest({ seed: "demo-multi" });

// Each test starts demo-multi and waits for its first sync before acting.
test.describe.configure({ timeout: 90_000 });

const moveWindow = (page: Page) => page.getByRole("dialog", { name: "Move to folder" });

/** The subjects of a mailbox of the work account on the mock IMAP server. */
const subjectsIn = async (mailbox: string) =>
  (await serverMessages(server, mailbox)).map((m) => m.subject);

/** How many Lab move messages a mailbox of the work account holds. */
const labIn = async (mailbox: string) =>
  (await subjectsIn(mailbox)).filter((s) => s.includes("Lab move")).length;

/** Opens the app, selects the Lab move conversation and opens the move window with `v`. */
async function openMoveForLab(page: Page) {
  await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS, size: WIDE_WINDOW });
  await messageRow(page, LAB).click();
  await expect(messageRow(page, LAB)).toHaveAttribute("aria-selected", "true");
  await messageList(page).focus();
  await page.keyboard.press("v");
  const move = moveWindow(page);
  await expect(move).toBeVisible();
  const filter = move.getByRole("combobox", { name: "Find a folder" });
  await expect(filter).toBeFocused();
  return { move, filter };
}

test("Enter moves only to the highlighted folder, all from the keyboard", async ({ page }) => {
  const { move, filter } = await openMoveForLab(page);
  // A partial match is not a choice: Enter says how to choose and moves nothing.
  await page.keyboard.type("arch");
  await page.keyboard.press("Enter");
  await expect(move.getByRole("status")).toHaveText(
    "Choose a folder with the arrow keys, then press Enter.",
  );
  await expect(move).toBeVisible();
  await expect(move.getByRole("option", { selected: true })).toHaveCount(0);

  // The arrow keys highlight an option, visibly.
  await page.keyboard.press("ArrowDown");
  const archive = move.getByRole("option", { name: "Archive", exact: true });
  await expect(archive).toHaveAttribute("aria-selected", "true");
  await expect(filter).toHaveAttribute(
    "aria-activedescendant",
    (await archive.getAttribute("id")) ?? "",
  );
  const background = (selector: string) =>
    move.locator(selector).evaluate((el) => getComputedStyle(el).backgroundColor);
  const highlighted = await background(
    '[data-part="folder-picker.item"][aria-selected="true"] [data-part="folder-picker.button"]',
  );
  expect(highlighted).not.toBe("rgba(0, 0, 0, 0)");

  await page.keyboard.press("Enter");
  await expect(move).toBeHidden();
  await expect.poll(() => labIn("Archive"), { timeout: 20_000 }).toBe(LAB_INBOX);
});

test("moving into Drafts asks first with the count; Cancel moves nothing", async ({ page }) => {
  const { move, filter } = await openMoveForLab(page);
  // The exact name is highlighted at once.
  await page.keyboard.type("drafts");
  await expect(move.getByRole("option", { name: "Drafts", exact: true })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await page.keyboard.press("Enter");
  const question = move.getByRole("group", {
    name: `Move ${String(LAB_INBOX)} messages to Drafts?`,
  });
  await expect(question).toContainText("opens in the reader");
  const cancel = question.getByRole("button", { name: "Cancel" });
  await expect(cancel).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(question).toBeHidden();
  await expect(filter).toBeFocused();
  expect(await labIn("Drafts")).toBe(0);

  // Asked again, Move moves exactly the counted messages.
  await page.keyboard.press("Enter");
  await expect(question).toBeVisible();
  await page.keyboard.press("Shift+Tab");
  await expect(question.getByRole("button", { name: "Move", exact: true })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(move).toBeHidden();
  await expect.poll(() => labIn("Drafts"), { timeout: 20_000 }).toBe(LAB_INBOX);
});

test("a received message in Drafts opens in the reader and moves out of Drafts", async ({
  page,
}) => {
  await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS, size: WIDE_WINDOW });
  // Received mail that ended up in Drafts: no \Draft flag.
  await control(server, "imap-deliver", {
    mailbox: "Drafts",
    raw: [
      "From: Grace Hopper <grace@navy.example>",
      "To: Ada Lovelace <ada@work.example>",
      "Subject: Moved here by mistake",
      "Message-ID: <mistake-1@navy.example>",
      "Date: Thu, 01 Oct 2026 09:00:00 +0000",
      "",
      "Not a draft at all.",
      "",
    ].join("\r\n"),
  });
  await page.locator('[data-part="folder-tree.favorite"][data-favorite="drafts"]').click();
  await refreshList(page);
  const row = messageRow(page, "Moved here by mistake");
  await expect(row).toBeVisible({ timeout: 20_000 });
  await row.dblclick();
  // The reader shows it; no composer opens, nothing replaces it.
  await expect(
    page.getByRole("feed").or(page.locator('[data-part="message-view"]')).first(),
  ).toContainText("Not a draft at all.");
  await expect(composerWindow(page)).toHaveCount(0);
  expect(await subjectsIn("Drafts")).toContain("Moved here by mistake");

  await readerAction(page, "Move out of Drafts");
  await expect
    .poll(() => subjectsIn("INBOX"), { timeout: 20_000 })
    .toContain("Moved here by mistake");
  expect(await subjectsIn("Drafts")).not.toContain("Moved here by mistake");
});
