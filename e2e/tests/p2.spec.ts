import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { buildOutputDir, nailPerTest } from "../support/nail";
import { rpcOk } from "../support/remote";
import { expectNoIssuesIn } from "../support/responsive";
import {
  composerWindow,
  discardComposer,
  messageRows,
  openMail,
  DEMO_MULTI_CONVERSATIONS,
  WIDE_WINDOW,
  messageList,
  messageRow,
} from "../support/views";
import { DINNER, LAB, LAB_TOTAL, READING } from "../support/demo";

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

const server = nailPerTest({ seed: "demo-multi" });

const feed = (page: Page) => page.getByRole("feed");
/** The message cards (the fold "N older messages" is an article of its own). */
const cards = (page: Page) => feed(page).locator('[data-part="thread-view.card"]');
const fold = (page: Page) => page.locator('[data-part="thread-view.fold"]');
/** The state words of an element (`data-state="expanded unread"`). */
const hasState = (state: string) => new RegExp(`(^|\\s)${state}(\\s|$)`);

const openApp = (page: Page, size = WIDE_WINDOW) =>
  openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS, size });

/** Selects a conversation row; returns the row and the reader's feed. */
async function openConversation(page: Page, subject: string) {
  const row = messageRow(page, subject);
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
    const lab = messageRow(page, LAB);
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
    const dinner = messageRow(page, DINNER);
    await expect(dinner.locator('[data-part="message-list.row.count"]')).toHaveText("4");
    const me = dinner.locator('[data-part="message-list.row.participant"]', { hasText: "me" });
    await expect(me).toHaveAttribute("data-state", hasState("me"));
    await expect(dinner).toHaveAttribute("data-state", hasState("unread"));

    // A conversation of one message has no count.
    const single = messageRow(page, "Welcome to Nail");
    await expect(single.locator('[data-part="message-list.row.count"]')).toBeHidden();

    // Four people take part in the reading group.
    await expect(
      messageRow(page, READING).locator('[data-part="message-list.row.count"]'),
    ).toHaveText("5");
  });

  test("names stay whole at ordinary window sizes: fewer names, not fragments", async ({
    page,
  }) => {
    for (const size of [
      { width: 1280, height: 800 },
      { width: 1024, height: 700 },
    ]) {
      await openApp(page, size);
      const cut = await page
        .locator('[data-part="message-list.row.participant"]')
        .evaluateAll((names) =>
          names.filter((n) => n.scrollWidth > n.clientWidth + 1).map((n) => n.textContent),
        );
      // At most a lone name may be cut (the newest sender, or "me" beside it).
      const rowsWithCuts = await messageRows(page).evaluateAll(
        (all) =>
          all.filter((row) => {
            const names = [
              ...row.querySelectorAll<HTMLElement>(
                '[data-part="message-list.row.participants"] [data-part="message-list.row.participant"]',
              ),
            ];
            return names.length > 2 && names.some((n) => n.scrollWidth > n.clientWidth + 1);
          }).length,
      );
      expect(rowsWithCuts, `${String(size.width)}: ${cut.join(" | ")}`).toBe(0);
      // First names when a row names several people.
      await expect(
        messageRow(page, LAB).locator('[data-part="message-list.row.participant"]').first(),
      ).toHaveText("Linus");
    }
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
    const lab = messageRow(page, LAB);
    await expect(lab.locator('[data-part="message-list.row.count"]')).toBeVisible();
    await expect(lab.locator('[data-part="message-list.row.participants"]')).toBeVisible();
    // The preview line is hidden: two lines remain.
    await expect(lab.locator('[data-part="message-list.row.preview"]')).toBeHidden();
    const lines = lab.locator('[data-part="message-list.row.line"]');
    await expect(lines.nth(2)).toBeHidden();
    await expect(lines.nth(0)).toBeVisible();
    await expect(lines.nth(1)).toBeVisible();
  });

  test("the Sent folder lists the user's own replies as their conversations", async ({ page }) => {
    await openApp(page);
    const work = page.locator('[data-part="folder-tree.section"][data-account="work"]');
    await work.locator('[data-part="folder-tree.item"]', { hasText: "Sent" }).first().click();
    // Nightingale, Dinner, Lab move, Reading group, Talk proposal.
    await expect(messageRows(page)).toHaveCount(5);
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

    // The unread message (Marie's, the 8th) and the newest one (the row's
    // choice) are open, the read ones around them collapsed to one line.
    const open = cards(page).and(page.locator('[data-state~="expanded"]'));
    await expect(open).toHaveCount(2);
    // Marie's card (opening marks it read, so its `unread` state goes).
    const unread = open.filter({ hasText: "calibration by the vendor's technician" });
    await expect(unread).toHaveCount(1);
    await expect(unread.locator('[data-part="message-view"]')).toBeVisible();
    await expect(cards(page).last()).toHaveAttribute("data-state", hasState("expanded"));
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
    const toggleOf = (card: Locator) => card.locator('[data-part="thread-view.card.toggle"]');
    await toggleOf(first).focus();
    await expect(toggleOf(first)).toBeFocused();
    // j / k: next and previous card (their header buttons take the focus).
    await page.keyboard.press("j");
    await expect(toggleOf(cards(page).nth(1))).toBeFocused();
    await page.keyboard.press("k");
    await expect(toggleOf(first)).toBeFocused();
    // Enter / o: open and close the focused card.
    await page.keyboard.press("Enter");
    await expect(first).toHaveAttribute("data-state", hasState("expanded"));
    await page.keyboard.press("o");
    await expect(first).not.toHaveAttribute("data-state", hasState("expanded"));
    // ; and Shift+; expand and collapse all.
    await page.keyboard.press(";");
    await expect(cards(page).and(page.locator('[data-state~="expanded"]'))).toHaveCount(LAB_TOTAL);
    // Shift+; on a keyboard types ":" (Playwright's "Shift+;" keeps the key ";").
    await page.keyboard.press(":");
    expect(
      await cards(page).and(page.locator('[data-state~="expanded"]')).count(),
    ).toBeLessThanOrEqual(1);
    // r answers the focused card's message (Marie's, the 8th).
    const marie = cards(page).nth(7);
    await toggleOf(marie).focus();
    await page.keyboard.press("r");
    const composer = composerWindow(page);
    await expect(composer).toBeVisible();
    await expect(composer.getByRole("textbox", { name: "Subject" })).toHaveValue(`Re: ${LAB}`);
    await expect(composer).toContainText("marie@radium.example");
    await discardComposer(page, composer);
  });

  test("from the list, Enter goes to the open card; the fold opens from the keyboard", async ({
    page,
  }) => {
    await openApp(page);
    const row = await openConversation(page, LAB);
    const toggleOf = (card: Locator) => card.locator('[data-part="thread-view.card.toggle"]');
    // The list has the focus after the click: Enter moves it to the
    // conversation's selected (open) card, the newest, without Tab stops.
    await expect(row).toHaveAttribute("data-state", hasState("selected"));
    await messageList(page).focus();
    await page.keyboard.press("Enter");
    await expect(toggleOf(cards(page).last())).toBeFocused();
    // Back to the list, o does the same.
    await messageList(page).focus();
    await page.keyboard.press("o");
    await expect(toggleOf(cards(page).last())).toBeFocused();

    // The fold is a line between two cards, not part of a card.
    await expect(fold(page)).toHaveCount(1);
    await expect(
      page.locator('[data-part="thread-view.card"] [data-part="thread-view.fold"]'),
    ).toHaveCount(0);
    await expect(fold(page).locator("xpath=..")).toHaveAttribute(
      "data-part",
      "thread-view.fold-row",
    );
    // Opening it from the keyboard keeps the focus in the feed: on the first
    // card it revealed, so j goes on from there.
    const before = await cards(page).count();
    await fold(page).focus();
    await page.keyboard.press("Enter");
    await expect(cards(page)).toHaveCount(LAB_TOTAL);
    const revealed = cards(page).nth(1);
    await expect(toggleOf(revealed)).toBeFocused();
    expect(before).toBeLessThan(LAB_TOTAL);
    await page.keyboard.press("j");
    await expect(toggleOf(cards(page).nth(2))).toBeFocused();
  });

  test("an open card is as tall as its message: the quote link sits under the text", async ({
    page,
  }) => {
    await openApp(page);
    await openConversation(page, LAB);
    const newest = cards(page).last();
    await expect(newest).toHaveAttribute("data-state", hasState("expanded"));
    const text = newest.locator('[data-part="message-view.text-block"]').first();
    const toggle = newest.locator('[data-part="message-view.quote-toggle"]').first();
    await expect(toggle).toBeVisible();
    const textBox = await text.boundingBox();
    const toggleBox = await toggle.boundingBox();
    const cardBox = await newest.boundingBox();
    if (textBox === null || toggleBox === null || cardBox === null) throw new Error("no boxes");
    // "Show quoted text" follows the text closely (it floated ~270 px below
    // it when the last card stretched to the reader's height), and the card
    // ends shortly after it.
    expect(toggleBox.y - (textBox.y + textBox.height)).toBeLessThan(40);
    expect(cardBox.y + cardBox.height - (toggleBox.y + toggleBox.height)).toBeLessThan(60);
  });

  test("the feed keeps to the responsive standard", async ({ page }) => {
    for (const size of [
      { width: 1600, height: 1000 },
      { width: 1024, height: 700 },
    ]) {
      await openApp(page, size);
      await openConversation(page, LAB);
      await page.getByRole("button", { name: "Expand all" }).click();
      await expectNoIssuesIn(page, ["thread-view", "message-list"]);
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
  '                row "{latest_from | names(1) | then(\\":\\")}{preview}"',
  "            }",
  '            stack id="reader" sizes="auto 1fr" {',
  '                toolbar id="reader-toolbar" toolbar="reader"',
  '                thread-view id="thread" mode="cards"',
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
    const lab = messageRow(page, LAB);
    const expander = lab.locator('[data-part="message-list.row.expander"]');
    // A pointer-only chevron: keyboard users press → (the row is a treeitem
    // with aria-expanded), so screen readers skip it.
    await expect(expander).toHaveAttribute("aria-hidden", "true");
    await expect(lab).toHaveAttribute("aria-expanded", "false");
    const children = page.locator('[data-part="message-list.child"]');
    await expect(children).toHaveCount(0);

    // The chevron opens the conversation: one child per message.
    await expander.click();
    await expect(lab).toHaveAttribute("data-state", hasState("expanded"));
    await expect(children).toHaveCount(LAB_TOTAL);
    // Child rows are roomy: about 40 px with body-size text (P2 review: they
    // were 27 px with small text, below the pointer minimum).
    const childBox = await children.first().boundingBox();
    expect(childBox?.height ?? 0).toBeGreaterThanOrEqual(36);
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

  test("rows keep the preview line, and the open list keeps to the responsive standard", async ({
    page,
  }) => {
    for (const size of [
      { width: 1600, height: 1000 },
      { width: 1024, height: 700 },
      { width: 800, height: 700 },
    ]) {
      await openApp(page, size);
      const lab = messageRow(page, LAB);
      // "Sender: preview" as in Design A rows.
      await expect(lab.locator('[data-part="message-list.row.preview"]')).toContainText(
        "All my crates are labelled",
      );
      await lab.locator('[data-part="message-list.row.expander"]').click();
      await expect(page.locator('[data-part="message-list.child"]')).toHaveCount(LAB_TOTAL);
      await expectNoIssuesIn(page, ["message-list"]);
    }
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
        // The newest card is open (and, on the first visit, the unread one).
        await expect(cards(page).last()).toHaveAttribute("data-state", hasState("expanded"));
        await page.screenshot({
          path: screenshotPath(
            `conversation-${theme}-${String(size.width)}x${String(size.height)}`,
          ),
        });
      }
    });
  }
});
