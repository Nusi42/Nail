import { readFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { control, serverMessages, startNail, type NailServer } from "../support/nail";
import {
  accountSectionNamed,
  openSettings as openSection,
  openSettingsTab,
  readerAction,
  runFromPalette,
  sectionInbox,
  showView,
} from "../support/views";

/**
 * Wave M2c (ADR 0048) against the `demo-multi` mock servers: server-side
 * rules and the vacation reply on the mock ManageSieve server, junk
 * training and filing, snooze and new-mail notifications under the core's
 * adjustable clock (`clock-set` / `clock-advance`), a headers-only folder
 * (server search, body fetched on open), context menus and a rebound key.
 */

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo-multi" });
});

test.afterEach(async () => {
  await server.stop();
});

interface SieveScripts {
  active: string | null;
  uploads: number;
  scripts: { name: string; active: boolean; content: string }[];
}

const sieveScripts = () => control<SieveScripts>(server, "sieve-scripts");
const nailScript = async () =>
  (await sieveScripts()).scripts.find((s) => s.name === "nail")?.content ?? "";

async function open(page: Page, rows = 17) {
  await page.goto(server.url);
  await expect(page.locator('[data-part="message-list.row"]')).toHaveCount(rows);
}

function deliver(subject: string, body: string, id: string) {
  return control(server, "imap-deliver", {
    raw: `From: Someone <someone@example.net>\r\nTo: ada@work.example\r\nSubject: ${subject}\r\nMessage-ID: <${id}@example.net>\r\n\r\n${body}\r\n`,
  });
}

/** The settings view at the section of a former settings tab (M5b). */
function openSettings(page: Page, tab: string): Promise<Locator> {
  return openSettingsTab(page, tab);
}

async function closeDialog(page: Page, name: string) {
  const dialog = page.getByRole("dialog", { name });
  await dialog.getByRole("button", { name: "Close" }).first().click();
  await expect(dialog).toBeHidden();
}

const rows = (page: Page) => page.locator('[data-part="message-list.row"]');

test("a rule runs on the server and the vacation reply is uploaded", async ({ page }) => {
  await open(page);
  // A rule, saved in the rule editor's text view.
  await runFromPalette(page, "filter rules", "Filter rules");
  const rules = page.getByRole("dialog", { name: "Filter rules" });
  await rules.getByRole("button", { name: "New rule" }).click();
  const editor = rules.getByRole("form", { name: "Rule" });
  await editor.getByRole("button", { name: "Text" }).click();
  await editor
    .getByRole("textbox", { name: "The rule in rules.kdl syntax" })
    .fill(
      'rule "Receipts" trigger="arrival" {\n    when {\n        subject contains="receipt"\n    }\n    then {\n        archive\n    }\n}\n',
    );
  await editor.getByRole("button", { name: "Editor" }).click();
  await editor.getByRole("button", { name: "Save rule" }).click();
  await expect(editor.getByText("The rule is saved.")).toBeVisible();
  await closeDialog(page, "Filter rules");

  // Settings → Server rules: turn ManageSieve on for the account.
  const settings = await openSettings(page, "Server rules");
  // The Server rules & vacation section: the ManageSieve card.
  const section = settings.locator('[data-part="sieve-status"]');
  await section.getByRole("checkbox", { name: "Run rules on the server when possible" }).check();
  await section.getByRole("button", { name: "Save", exact: true }).click();
  await expect(section.getByText("Up to date on the server")).toBeVisible({ timeout: 20_000 });
  const placement = section.getByRole("row", { name: /^Receipts/ });
  await expect(placement.getByRole("cell").first()).toHaveText("Server", { timeout: 20_000 });
  expect((await sieveScripts()).active).toBe("nail");
  expect(await nailScript()).toContain("receipt");

  // The vacation reply lands in the same script.
  await openSettingsTab(page, "Vacation reply");
  const vacation = settings.locator('[data-part="vacation-editor"]');
  await vacation.getByRole("checkbox", { name: "Send a vacation reply" }).check();
  await vacation.getByRole("textbox", { name: "Message" }).fill("I am away until Monday.");
  await vacation.getByRole("button", { name: "Save", exact: true }).click();
  await expect(vacation.getByText("Saved. Nail uploads the reply with your rules.")).toBeVisible();
  await expect.poll(nailScript, { timeout: 20_000 }).toContain("I am away until Monday.");
  expect(await nailScript()).toContain("vacation");
  await openSettingsTab(page, "Server rules");
  await expect(section.getByText("Active on the server")).toBeVisible({ timeout: 20_000 });
});

test("marking junk trains the filter and the next similar message lands in Junk", async ({
  page,
}) => {
  const spam =
    "Congratulations winner! Claim your lottery prize money now, cheap pills, exclusive casino bonus, click here to claim the prize today";
  await deliver("You are a lottery winner", spam, "spam-1");
  await open(page, 18);
  await expect(rows(page).filter({ hasText: "You are a lottery winner" })).toHaveCount(1, {
    timeout: 20_000,
  });

  // Nail's filter on, trained after one message of each kind.
  const settings = await openSettings(page, "Junk mail");
  const junk = settings.locator('[data-part="junk-settings"]');
  await junk
    .getByRole("checkbox", { name: "Classify arriving mail with Nail's junk filter" })
    .check();
  await junk.getByRole("spinbutton", { name: "Junk threshold (percent)" }).fill("60");
  await junk.getByRole("spinbutton", { name: "Training needed" }).fill("1");
  await junk.getByRole("button", { name: "Save", exact: true }).click();
  await expect(junk.getByText("Saved.", { exact: true })).toBeVisible();
  await showView(page, "Mail");

  // "Not junk" for a normal message: it applies only to mail in Junk (M5b
  // review), so the welcome mail goes to Junk and comes back from there
  // through its context menu — the filter learns it as wanted mail…
  const normal = rows(page).filter({ hasText: "Welcome to Nail" });
  await normal.click({ button: "right" });
  const menu = page.getByRole("menu");
  await expect(menu.getByRole("menuitem", { name: "Not junk" })).toHaveCount(0);
  await menu.getByRole("menuitem", { name: "Mark as junk" }).click();
  await expect(normal).toHaveCount(0);
  const work = accountSectionNamed(page, "Work");
  await work.locator('[data-part="folder-tree.item"][data-role="junk"]').first().click();
  const inJunk = rows(page).filter({ hasText: "Welcome to Nail" });
  await inJunk.click({ button: "right" });
  await expect(menu.getByRole("menuitem", { name: "Mark as junk" })).toHaveCount(0);
  await menu.getByRole("menuitem", { name: "Not junk" }).click();
  await expect(inJunk).toHaveCount(0);
  await sectionInbox(work).click();
  await expect(normal).toHaveCount(1);
  // …and "Mark as junk" for the spam (reader toolbar).
  await rows(page).filter({ hasText: "You are a lottery winner" }).click();
  await readerAction(page, "Mark as junk");
  await expect(rows(page).filter({ hasText: "You are a lottery winner" })).toHaveCount(0);
  await expect
    .poll(async () => (await serverMessages(server, "Junk")).map((m) => m.subject), {
      timeout: 20_000,
    })
    .toContain("You are a lottery winner");

  // The next similar message is filed as junk by the classifier.
  await deliver("Another lottery winner", spam, "spam-2");
  await expect
    .poll(async () => (await serverMessages(server, "Junk")).map((m) => m.subject), {
      timeout: 20_000,
    })
    .toContain("Another lottery winner");
  await expect(rows(page).filter({ hasText: "Another lottery winner" })).toHaveCount(0);
});

test("a snoozed message hides and returns at the wake time", async ({ page }) => {
  await open(page);
  const row = rows(page).filter({ hasText: "Welcome to Nail" });
  await row.click();
  await readerAction(page, "Snooze…");
  const picker = page.getByRole("dialog", { name: "Snooze" });
  await picker.getByRole("button", { name: /^Tomorrow/ }).click();
  await expect(picker).toBeHidden();
  await expect(row).toHaveCount(0);
  await expect
    .poll(async () => (await serverMessages(server, "Snoozed")).map((m) => m.subject), {
      timeout: 20_000,
    })
    .toContain("Welcome to Nail");

  // The snoozed list shows it.
  await runFromPalette(page, "snoozed", "Snoozed messages");
  const list = page.getByRole("dialog", { name: "Snoozed messages" });
  await expect(list.getByText("Welcome to Nail")).toBeVisible();
  await closeDialog(page, "Snoozed messages");

  // Two days later (the core's clock) it is back.
  await control(server, "clock-advance", { seconds: 2 * 86_400 });
  await expect(row).toHaveCount(1, { timeout: 20_000 });
  await expect
    .poll(async () => (await serverMessages(server, "INBOX")).map((m) => m.subject), {
      timeout: 20_000,
    })
    .toContain("Welcome to Nail");
});

test("new mail notifies, and quiet hours suppress it", async ({ page, context }) => {
  // The browser's notifications, recorded (the remote UI shows them).
  await context.grantPermissions(["notifications"]);
  await page.addInitScript(() => {
    const shown: string[] = [];
    (window as unknown as { nailNotifications: string[] }).nailNotifications = shown;
    class RecordingNotification {
      static permission = "granted";
      static requestPermission = () => Promise.resolve("granted");
      onclick: (() => void) | null = null;
      constructor(title: string, options?: { body?: string }) {
        shown.push(`${title}|${options?.body ?? ""}`);
      }
      close() {
        /* no-op */
      }
    }
    Object.defineProperty(window, "Notification", {
      value: RecordingNotification,
      configurable: true,
    });
  });
  const shown = () =>
    page.evaluate(() => (window as unknown as { nailNotifications: string[] }).nailNotifications);
  // The core's clock at noon today (local time of this machine).
  const noon = new Date();
  noon.setHours(12, 0, 0, 0);
  await control(server, "clock-set", { at: Math.floor(noon.getTime() / 1000) });
  await open(page);

  await deliver("Lunch on Friday?", "Shall we?", "lunch-1");
  await expect.poll(shown, { timeout: 20_000 }).toHaveLength(1);
  expect((await shown())[0]).toContain("Lunch on Friday?");

  // Quiet hours around noon: the next mail does not notify.
  const settings = await openSettings(page, "Notifications");
  const form = settings.locator('[data-part="notification-settings"]');
  await form.getByRole("checkbox", { name: "No notifications during quiet hours" }).check();
  await form.getByLabel("From").fill("11:00");
  await form.getByLabel("Until").fill("13:00");
  await form.getByRole("button", { name: "Save", exact: true }).click();
  await expect(form.getByText("Saved.", { exact: true })).toBeVisible();
  await showView(page, "Mail");
  await deliver("Quiet please", "Shh.", "quiet-1");
  await expect(rows(page).filter({ hasText: "Quiet please" })).toHaveCount(1, { timeout: 20_000 });
  await page.waitForTimeout(4_000);
  expect(await shown()).toHaveLength(1);
});

test("server search finds mail without a local body, and opening fetches it", async ({ page }) => {
  await open(page);
  // The folder is synced headers only.
  const newsletters = page.getByRole("treeitem", { name: /^Newsletters/ });
  await expect(newsletters).toBeVisible({ timeout: 20_000 });
  await newsletters.click();
  const row = rows(page).filter({ hasText: "This week in nature" });
  await expect(row).toHaveCount(1);

  // "platypus" is only in the body, which is not stored locally: the
  // server search finds it. (Search hits are message rows; the folder lists
  // conversations, P2.)
  const search = page.getByRole("searchbox");
  await search.fill("platypus");
  await search.press("Enter");
  await expect(row).toHaveCount(1, { timeout: 20_000 });
  await expect(rows(page)).toHaveCount(1);
  await expect(row).toHaveAttribute("data-state", /headers-only/);

  // Opening it fetches the body.
  await row.click();
  await expect(page.locator('[data-part="message-view"]')).toContainText("platypus", {
    timeout: 20_000,
  });
  await expect(row).not.toHaveAttribute("data-state", /headers-only/);
});

test("the right-click menu of a message works, also from the keyboard", async ({ page }) => {
  await open(page);
  const row = rows(page).filter({ hasText: "Welcome to Nail" });
  await row.click({ button: "right" });
  const menu = page.getByRole("menu");
  await expect(menu).toBeVisible();
  await menu.getByRole("menuitem", { name: "Flag", exact: true }).click();
  await expect(menu).toBeHidden();
  await expect(row).toHaveAttribute("data-state", /flagged/);
  await expect
    .poll(async () =>
      (await serverMessages(server, "INBOX"))
        .find((m) => m.subject === "Welcome to Nail")
        ?.flags.includes("\\Flagged"),
    )
    .toBe(true);

  // Shift+F10 opens the same menu for the selected row; Escape closes it.
  await row.click();
  await page.keyboard.press("Shift+F10");
  await expect(page.getByRole("menu")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("menu")).toBeHidden();
});

test("a rebound key is written to layout.kdl and works", async ({ page }) => {
  await open(page);
  // M5b: the shortcut editor has its own settings section.
  const keys = await openSection(page, "Keyboard shortcuts");
  await keys.getByRole("combobox", { name: "Where" }).selectOption("global");
  await keys.getByRole("combobox", { name: "Action" }).selectOption("mail.toggle-flag");
  const capture = keys.getByRole("textbox", { name: "Keys" });
  await capture.focus();
  await page.keyboard.press("Control+Shift+Y");
  await expect(capture).toHaveValue("Ctrl+Shift+Y");
  await keys.getByRole("button", { name: "Add shortcut" }).click();
  await expect(keys.getByText("Shortcut saved to layout.kdl.")).toBeVisible();
  const layout = readFileSync(path.join(server.configDir, "layout.kdl"), "utf8");
  expect(layout).toContain("ctrl+shift+y");
  await showView(page, "Mail");

  const row = rows(page).filter({ hasText: "Welcome to Nail" });
  await row.click();
  await page.keyboard.press("Control+Shift+Y");
  await expect(row).toHaveAttribute("data-state", /flagged/);
});
