import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { buildOutputDir, startNail, type NailServer } from "../support/nail";
import { rpcOk } from "../support/remote";
import { formatIssues, layoutIssues } from "../support/responsive";
import { composerWindow, discardComposer } from "../support/views";

/**
 * P2 (conversations, ADR 0148) on the real core with `--seed demo-multi`:
 * Design A — one list row per conversation (participants, count pill, the
 * newest sender's preview), the reader as a card feed (oldest first, the
 * newest unread card open, read cards collapsed, the middle folded into
 * "N older messages", Expand all / Collapse all, keys), the user's own
 * replies from Sent inside the conversation, marked `from-me` — and
 * Design B, the opt-in `threads="expandable"` rows that open into their
 * messages.
 *
 * The demo corpus's conversations (`nail_testkit::DEMO_CONVERSATIONS`):
 * "Lab move: timeline and packing" — 12 messages (9 in the Inbox, the
 * user's 3 replies in Sent), five people, the 8th message (Marie Curie's)
 * unread, the newest (Ada Lovelace's) read; "Dinner on Saturday?" — started
 * by the user in Sent, 4 messages between two people, the newest unread;
 * "Reading group: October book" — 5 messages, four people.
 *
 * Screenshots for the owner (light Frost, dark Graphite and Nocturne;
 * 1600 × 1000 and 1024 × 700; the expandable list): `<build dir>/screens/p2/`.
 */

const LAB = "Lab move: timeline and packing";
const DINNER = "Dinner on Saturday?";
const READING = "Reading group: October book";
/** Messages of the Lab move conversation: 9 in the Inbox + 3 replies in Sent. */
const LAB_TOTAL = 12;

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo-multi" });
});

test.afterEach(async () => {
  await server.stop();
});

const rows = (page: Page) => page.locator('[data-part="message-list.row"]');
const rowOf = (page: Page, subject: string) => rows(page).filter({ hasText: subject });
const feed = (page: Page) => page.getByRole("feed");
const cards = (page: Page) => feed(page).getByRole("article");
const fold = (page: Page) => page.locator('[data-part="thread-view.fold"]');
/** The state words of an element (`data-state="expanded unread"`). */
const hasState = (state: string) => new RegExp(`(^|\\s)${state}(\\s|$)`);

async function openApp(page: Page, size = { width: 1600, height: 1000 }) {
  await page.setViewportSize(size);
  await page.goto(server.url);
  // 32 messages in the unified inbox are 17 conversations (14 before P2's
  // demo threads: Lab move, Dinner, Reading group).
  await expect(rows(page)).toHaveCount(17);
}

/** Selects a conversation row; returns the row and the reader's feed. */
async function openConversation(page: Page, subject: string) {
  const row = rowOf(page, subject);
  await expect(row).toHaveCount(1);
  await row.click();
  await expect(feed(page)).toBeVisible();
  await expect(page.locator('[data-part="thread-view.subject"]')).toContainText(subject);
  return row;
}

/** Shows every card: opens the fold when there is one. */
async function unfold(page: Page) {
  if ((await fold(page).count()) > 0) await fold(page).click();
  await expect(cards(page)).toHaveCount(LAB_TOTAL);
}

function screenshotPath(name: string): string {
  const dir = buildOutputDir(path.join("screens", "p2"));
  mkdirSync(dir, { recursive: true });
  return path.join(dir, `${name}.png`);
}

/** The text of a card's header (sender, date). */
const cardHeader = (card: Locator) => card.locator('[data-part="thread-view.card.header"]');

test.describe("conversation rows (Design A)", () => {
  test("a row shows who takes part, the count and the newest sender's preview", async ({
    page,
  }) => {
    await openApp(page);
    const lab = rowOf(page, LAB);
    await expect(lab).toHaveCount(1);
    await expect(lab).toHaveAttribute("data-state", hasState("thread"));
    // The count covers the Inbox and the user's replies from Sent (WP201).
    await expect(lab.locator('[data-part="message-list.row.count"]')).toHaveText(String(LAB_TOTAL));
    // At most three names: the first ones and the newest; Marie Curie sent
    // unread mail, so her name is bold (state `unread`).
    const participants = lab.locator('[data-part="message-list.row.participants"]');
    await expect(participants).toContainText("Linus");
    await expect(participants).toContainText("Ada");
    const names = participants.locator('[data-part="message-list.row.participant"]');
    expect(await names.count()).toBeLessThanOrEqual(3);
    await expect(names.filter({ hasText: "Marie" })).toHaveAttribute(
      "data-state",
      hasState("unread"),
    );
    // "Sender: preview" of the newest message.
    await expect(lab.locator('[data-part="message-list.row.preview"]')).toContainText(
      "All my crates are labelled",
    );

    // The user is "me": the Dinner conversation started in Sent.
    const dinner = rowOf(page, DINNER);
    await expect(dinner.locator('[data-part="message-list.row.count"]')).toHaveText("4");
    const me = dinner.locator('[data-part="message-list.row.participant"]', { hasText: "me" });
    await expect(me).toHaveAttribute("data-state", hasState("me"));
    await expect(dinner).toHaveAttribute("data-state", hasState("unread"));

    // A conversation of one message has no count.
    const single = rowOf(page, "Welcome to Nail");
    await expect(single.locator('[data-part="message-list.row.count"]')).toBeHidden();

    // Four people take part in the reading group.
    await expect(rowOf(page, READING).locator('[data-part="message-list.row.count"]')).toHaveText(
      "5",
    );
  });

  test("Compact rows have two lines and no preview", async ({ page }) => {
    await rpcOk(
      server,
      {
        type: "SetThemePreferences",
        preferences: {
          follow_system: false,
          light: "frost",
          dark: "graphite",
          density: "compact",
          overrides: {},
        },
      },
      "Themes",
    );
    await openApp(page);
    const lab = rowOf(page, LAB);
    await expect(lab.locator('[data-part="message-list.row.count"]')).toBeVisible();
    await expect(lab.locator('[data-part="message-list.row.participants"]')).toBeVisible();
    await expect(lab.locator('[data-part="message-list.row.preview"]')).toHaveCount(0);
    await expect(lab.locator('[data-part="message-list.row.line"]')).toHaveCount(2);
  });

  test("the Sent folder lists the user's own replies as their conversations", async ({ page }) => {
    await openApp(page);
    const work = page.locator('[data-part="folder-tree.section"][data-account="work"]');
    await work.locator('[data-part="folder-tree.item"]', { hasText: "Sent" }).first().click();
    // Nightingale, Dinner, Lab move, Reading group, Talk proposal.
    await expect(rows(page)).toHaveCount(5);
    await openConversation(page, LAB);
    // Listed in Sent, the conversation is the user's three replies.
    await expect(cards(page)).toHaveCount(3);
    for (const card of await cards(page).all()) {
      await expect(card).toHaveAttribute("data-state", hasState("from-me"));
    }
  });
});

test.describe("the card feed (Design A)", () => {
  test("oldest first, the unread card open, read cards collapsed, the middle folded", async ({
    page,
  }) => {
    await openApp(page);
    await openConversation(page, LAB);
    const shown = await cards(page).count();
    // Every card knows the size of the feed and its place in it.
    await expect(cards(page).first()).toHaveAttribute("aria-setsize", String(LAB_TOTAL));
    await expect(cards(page).first()).toHaveAttribute("aria-posinset", "1");
    await expect(cards(page).last()).toHaveAttribute("aria-posinset", String(LAB_TOTAL));
    // Oldest first: Linus's announcement on top, Ada's last message below.
    await expect(cards(page).first()).toContainText("Linus");
    await expect(cards(page).last()).toContainText("Ada");

    // The read messages in the middle are folded ("N older messages").
    await expect(fold(page)).toHaveCount(1);
    const folded = LAB_TOTAL - shown;
    expect(folded).toBeGreaterThan(0);
    await expect(fold(page)).toHaveAccessibleName(`${String(folded)} older messages`);

    // The newest unread message is open (Marie's, the 8th), the read ones
    // around it are collapsed to one line.
    const open = cards(page).and(page.locator('[data-state~="expanded"]'));
    await expect(open).toHaveCount(1);
    await expect(open).toHaveAttribute("data-state", hasState("unread"));
    await expect(open).toContainText("calibration by the vendor's technician");
    await expect(open.locator('[data-part="message-view"]')).toBeVisible();
    const first = cards(page).first();
    await expect(first).not.toHaveAttribute("data-state", hasState("expanded"));
    const toggle = cardHeader(first).getByRole("button").first();
    await expect(toggle).toHaveAttribute("aria-expanded", "false");

    // Opening the fold shows every card.
    await fold(page).click();
    await expect(cards(page)).toHaveCount(LAB_TOTAL);
    await expect(fold(page)).toHaveCount(0);

    // A collapsed card opens in place and closes again.
    await toggle.click();
    await expect(first).toHaveAttribute("data-state", hasState("expanded"));
    await expect(first).toContainText("the lab moves to building C");
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    await toggle.click();
    await expect(first).not.toHaveAttribute("data-state", hasState("expanded"));
  });

  test("Expand all and Collapse all", async ({ page }) => {
    await openApp(page);
    await openConversation(page, LAB);
    await page.getByRole("button", { name: "Expand all" }).click();
    await expect(cards(page)).toHaveCount(LAB_TOTAL);
    await expect(cards(page).and(page.locator('[data-state~="expanded"]'))).toHaveCount(LAB_TOTAL);
    // Quoted text stays folded inside the open cards.
    await expect(cards(page).nth(9)).toContainText("The packing list is in the shared folder");
    await page.getByRole("button", { name: "Collapse all" }).click();
    expect(
      await cards(page).and(page.locator('[data-state~="expanded"]')).count(),
    ).toBeLessThanOrEqual(1);
  });

  test("the user's own replies from Sent are in the conversation, marked from-me", async ({
    page,
  }) => {
    await openApp(page);
    await openConversation(page, LAB);
    await unfold(page);
    const mine = cards(page).and(page.locator('[data-state~="from-me"]'));
    await expect(mine).toHaveCount(3);
    for (const card of await mine.all()) {
      await expect(cardHeader(card)).toContainText("me");
    }
    // The third message is the user's first reply.
    await expect(cards(page).nth(2)).toHaveAttribute("data-state", hasState("from-me"));

    // A conversation the user started: the first card is the user's.
    await openConversation(page, DINNER);
    await expect(cards(page)).toHaveCount(4);
    await expect(cards(page).first()).toHaveAttribute("data-state", hasState("from-me"));
    // The newest is unread and open.
    await expect(cards(page).last()).toHaveAttribute("data-state", hasState("expanded"));
    await expect(cards(page).last()).toContainText("I will bring the book I owe you");
  });

  test("keys move between cards, open them, expand and collapse all, and reply", async ({
    page,
  }) => {
    await openApp(page);
    await openConversation(page, LAB);
    await unfold(page);
    const first = cards(page).first();
    await first.focus();
    await expect(first).toBeFocused();
    // j / k: next and previous card.
    await page.keyboard.press("j");
    await expect(cards(page).nth(1)).toBeFocused();
    await page.keyboard.press("k");
    await expect(first).toBeFocused();
    // Enter / o: open and close the focused card.
    await page.keyboard.press("Enter");
    await expect(first).toHaveAttribute("data-state", hasState("expanded"));
    await page.keyboard.press("o");
    await expect(first).not.toHaveAttribute("data-state", hasState("expanded"));
    // ; and Shift+; expand and collapse all.
    await page.keyboard.press(";");
    await expect(cards(page).and(page.locator('[data-state~="expanded"]'))).toHaveCount(LAB_TOTAL);
    await page.keyboard.press("Shift+;");
    expect(
      await cards(page).and(page.locator('[data-state~="expanded"]')).count(),
    ).toBeLessThanOrEqual(1);
    // r answers the focused card's message (Marie's, the 8th).
    const marie = cards(page).nth(7);
    await marie.focus();
    await page.keyboard.press("r");
    const composer = composerWindow(page);
    await expect(composer).toBeVisible();
    await expect(composer.getByRole("textbox", { name: "Subject" })).toHaveValue(`Re: ${LAB}`);
    await expect(composer).toContainText("marie@radium.example");
    await discardComposer(page, composer);
  });

  test("the feed keeps to the responsive standard", async ({ page }) => {
    for (const size of [
      { width: 1600, height: 1000 },
      { width: 1024, height: 700 },
    ]) {
      await openApp(page, size);
      await openConversation(page, LAB);
      await page.getByRole("button", { name: "Expand all" }).click();
      const issues = (await layoutIssues(page)).filter(
        (i) => i.element.includes("thread-view") || i.element.includes("message-list"),
      );
      expect(issues, formatIssues(issues)).toEqual([]);
    }
  });
});

/** A user layout whose mail window lists conversations that open (Design B). */
const EXPANDABLE_LAYOUT = [
  'layout "Mine" extends="default" {',
  '    window "mail" {',
  '        split "horizontal" id="panes" sizes="260px 1fr 1.4fr" {',
  '            folder-tree id="folders" show-unread=#true sections="favorites accounts" \\',
  '                favorites="all-inboxes" collapsible=#true account-colors=#true',
  '            message-list id="list" page-size=100 threads="expandable" {',
  '                row "{participants | names(3)}  {count | if-many}  {date | relative}"',
  '                row "{subject}"',
  "            }",
  '            stack id="reader" sizes="auto auto 1fr" {',
  '                toolbar id="reader-toolbar" toolbar="reader"',
  '                thread-view id="thread"',
  '                message-view id="message" headers="from to cc date"',
  "            }",
  "        }",
  "    }",
  "}",
  "",
].join("\n");

test.describe("conversations that open in the list (Design B, opt-in)", () => {
  test.beforeEach(() => {
    writeFileSync(path.join(server.configDir, "layout.kdl"), EXPANDABLE_LAYOUT);
  });

  test("a row opens into its messages; a child shows just that message", async ({ page }) => {
    await openApp(page);
    const lab = rowOf(page, LAB);
    const expander = lab.locator('[data-part="message-list.row.expander"]');
    await expect(expander).toHaveAccessibleName("Show the conversation's messages");
    const children = page.locator('[data-part="message-list.child"]');
    await expect(children).toHaveCount(0);

    // The chevron opens the conversation: one child per message.
    await expander.click();
    await expect(lab).toHaveAttribute("data-state", hasState("expanded"));
    await expect(children).toHaveCount(LAB_TOTAL);
    // Marie's unread message ("One more thing: …") is unread among them.
    await expect(children.filter({ hasText: "One more thing" })).toHaveAttribute(
      "data-state",
      hasState("unread"),
    );
    await page.screenshot({ path: screenshotPath("expandable-frost-1600x1000") });

    // A child shows that message alone in the reader.
    const linus = children.first();
    await linus.click();
    await expect(linus).toHaveAttribute("data-state", hasState("selected"));
    await expect(page.locator('[data-part="message-view"]')).toContainText(
      "the lab moves to building C",
    );
    await expect(feed(page)).toHaveCount(0);

    // ← on the row closes it, → opens it again.
    await lab.click();
    await page.keyboard.press("ArrowLeft");
    await expect(children).toHaveCount(0);
    await page.keyboard.press("ArrowRight");
    await expect(children).toHaveCount(LAB_TOTAL);
  });
});

test.describe("screenshots for the owner", () => {
  for (const theme of ["frost", "graphite", "nocturne"] as const) {
    test(`the conversation list and reader in ${theme}`, async ({ page }) => {
      await rpcOk(server, { type: "SetTheme", theme_id: theme }, "Themes");
      for (const size of [
        { width: 1600, height: 1000 },
        { width: 1024, height: 700 },
      ]) {
        await openApp(page, size);
        await openConversation(page, LAB);
        await expect(cards(page).and(page.locator('[data-state~="expanded"]'))).toHaveCount(1);
        await page.screenshot({
          path: screenshotPath(
            `conversation-${theme}-${String(size.width)}x${String(size.height)}`,
          ),
        });
      }
    });
  }
});
