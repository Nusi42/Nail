/**
 * The screens of the visual-regression harness (plan M5a WP154, M5b): the
 * main window (rail + mail view) with a message open, the reader's "More"
 * menu, the composer, the settings page, the calendar's week, month, year,
 * agenda and free-time views, the Contacts and Tasks views, the main window
 * and the calendar at 1024 px (M5b review), a context menu, HTML and
 * attachment messages, a dialog and a modal confirmation (M5c), and the M5d
 * editors (customize layout, the toolbar customizer, Settings → Appearance,
 * the theme editor), the P2 conversation (its row and the card feed,
 * collapsed and expanded) and the connection log with an error's details
 * (ADR 0190). Each starts from a freshly
 * loaded main window (so one screen never leaks state into the next) and
 * ends with the screen open and its content present; `support.ts` then
 * settles and captures it.
 *
 * Dialogs are found by their layout window name
 * (`[data-part="window"][data-window=…]`), views by their region in the
 * view host ("<View> view", from the navigation rail), controls by role and
 * accessible name, as in the regular e2e suite.
 */
import { expect, type Locator, type Page } from "@playwright/test";
import {
  allowAllFolders,
  errorDetailsWindow,
  openWorkLog,
  otherMailbox,
  showRefusedFolderDetails,
  workLogWindow,
} from "../support/connlog";
import type { NailServer } from "../support/nail";
import { messageRows, READER_SUBJECT, runFromPalette, windowById } from "../support/views";

/**
 * The fixed clock of every screen but the calendar: after the demo mail's
 * dates. It is also the seeded run's pinned "now" (ADR 0185): the demo's
 * feeds and meetings are dated from its day, the core's clock starts at it.
 */
export const MAIN_TIME = new Date("2026-09-30T10:00:00Z");

/**
 * The calendar's fixed clock: weeks before the demo's own events (dated
 * from {@link MAIN_TIME}), so the day / week / month screens show only the
 * fixed events of {@link CALENDAR_EVENTS}; the year screen shows the demo's
 * events of October as well, on the same days in every run.
 */
export const CALENDAR_TIME = new Date("2026-08-12T10:00:00Z");

/** Events put on the mock CalDAV server (`personal`) for the calendar screen. */
export const CALENDAR_EVENTS = [
  {
    uid: "visual-design-review",
    title: "Design review",
    start: "20260810T090000Z",
    end: "20260810T100000Z",
  },
  {
    uid: "visual-lunch",
    title: "Lunch with Grace",
    start: "20260812T120000Z",
    end: "20260812T130000Z",
  },
  {
    uid: "visual-planning",
    title: "Release planning",
    start: "20260813T140000Z",
    end: "20260813T153000Z",
  },
] as const;

/** The message every screen that needs one selects. */
export const SAMPLE_MESSAGE = "Welcome to Nail";

export interface Screen {
  /** File name of the screenshot (`<id>.png`). */
  id: string;
  /** What the screen shows, for people reading the harness. */
  title: string;
  /** The fixed clock the page loads with (default {@link MAIN_TIME}). */
  time?: Date;
  /** The window size (default the harness's 1280 × 1000). */
  viewport?: { width: number; height: number };
  /** Opens the screen on a freshly loaded main window (`server`: its test controls). */
  open(page: Page, server: NailServer): Promise<void>;
  /** Undoes what `open` changed on the server, after the capture (the server is shared by the screens). */
  close?(page: Page, server: NailServer): Promise<void>;
  /** Parts that legitimately differ between runs (none so far). */
  mask?(page: Page): Locator[];
}

/** The navigation rail (M5b). */
export const rail = (page: Page) => page.getByRole("navigation", { name: "Sections", exact: true });

/** Shows a view from the rail; returns its region. */
async function showView(
  page: Page,
  name: "Calendar" | "Contacts" | "Tasks" | "Settings",
): Promise<Locator> {
  await rail(page)
    .getByRole("button", { name: new RegExp(`^${name}(,|$)`) })
    .click();
  const region = page.getByRole("region", { name: `${name} view`, exact: true });
  await expect(region).toBeVisible();
  return region;
}
/** A narrow window (M5b review: the views at 1024 px). */
const NARROW = { width: 1024, height: 900 } as const;

/** The calendar view with its fixed events of the week. */
async function calendarWeek(page: Page): Promise<Locator> {
  const calendar = await showView(page, "Calendar");
  for (const event of CALENDAR_EVENTS) {
    await expect(
      calendar.getByRole("button", { name: new RegExp(`^${event.title},`) }).first(),
    ).toBeVisible();
  }
  return calendar;
}

/**
 * Switches the calendar to a view through the segmented switch, or through
 * the view menu that replaces it when the toolbar is too narrow.
 */
async function chooseView(calendar: Locator, label: string, value: string): Promise<void> {
  const group = calendar.getByRole("group", { name: "View", exact: true });
  if (await group.isVisible()) {
    await group.getByRole("button", { name: label, exact: true }).click();
  } else {
    await calendar.getByRole("combobox", { name: "View", exact: true }).selectOption(value);
  }
  await expect(calendar.locator('[data-part="calendar"]').first()).toHaveAttribute(
    "data-view",
    value,
  );
}
export const layoutWindow = windowById;

/** Loads the app and waits until the demo mail is listed. */
export async function loadMainWindow(page: Page, url: string): Promise<void> {
  await page.goto(url);
  await expect(messageRows(page).filter({ hasText: SAMPLE_MESSAGE }).first()).toBeVisible();
  // Account states show (status dots); never capture one mid-connect.
  await expect(page.locator('[data-status="connecting"]')).toHaveCount(0, { timeout: 60_000 });
}

/** A demo newsletter with an HTML body and blocked remote images. */
export const HTML_MESSAGE = "Your weekly garden digest";
/** The demo invoice with a PDF and a CSV attachment. */
export const ATTACHMENT_MESSAGE = "Your invoice for September 2026";

/**
 * The demo's long conversation (P2, ADR 0148): 12 messages, five people, the
 * user's three replies from Sent, the 8th message unread.
 */
export const CONVERSATION = "Lab move: timeline and packing";
/** Search query for the conversation's unread message (Marie Curie's 8th). */
export const CONVERSATION_UNREAD = `subject:"${CONVERSATION}" from:marie@radium.example calibration`;

/**
 * Selects the long conversation: its row in the list, the reader's card
 * feed with the unread card and the newest open (P2 Design A).
 */
async function selectConversation(page: Page) {
  await messageRows(page).filter({ hasText: CONVERSATION }).first().click();
  const feed = page.getByRole("feed");
  await expect(feed).toBeVisible();
  await expect(page.locator(READER_SUBJECT)).toContainText(CONVERSATION);
  // The unread card (Marie's, the 8th) and the newest are open.
  await expect(
    feed.locator('[data-part="thread-view.card"][aria-posinset="8"][data-state~="expanded"]'),
  ).toContainText("calibration");
  await expect(
    feed.locator('[data-part="thread-view.card"][aria-posinset="12"][data-state~="expanded"]'),
  ).toBeVisible();
  await pinListScroll(page, CONVERSATION);
  return feed;
}

async function selectMessage(page: Page, subject: string) {
  await messageRows(page).filter({ hasText: subject }).first().click();
  await expect(page.locator(READER_SUBJECT)).toContainText(subject);
  await pinListScroll(page, subject);
}

/**
 * Waits until the message list stops scrolling: its scroll position is the
 * same over several animation frames (the virtualizer scrolls to the
 * selected row, measures the rows, and corrects over a few frames).
 */
async function listScrollSettled(page: Page): Promise<void> {
  await page
    .locator('[data-part="message-list"]')
    .first()
    .evaluate(async (list) => {
      const frame = () =>
        new Promise<void>((resolve) => {
          requestAnimationFrame(() => {
            resolve();
          });
        });
      let last = list.scrollTop;
      let same = 0;
      for (let i = 0; i < 120 && same < 5; i++) {
        await frame();
        same = list.scrollTop === last ? same + 1 : 0;
        last = list.scrollTop;
      }
    });
}

/**
 * Puts the message list at a scroll position that depends only on the
 * layout (P1 WP193): scrolled from the top just far enough to show the
 * selected row at its bottom edge, or not at all when the row is visible
 * from the top — where the virtualizer's "keep the selection visible"
 * aims. The virtualizer itself scrolls with estimated row heights
 * first and corrects after measuring, so where it stopped depended on
 * timing (High contrast · comfortable `main-1024` / `html` differed between
 * runs under load).
 */
async function pinListScroll(page: Page, subject: string): Promise<void> {
  const row = messageRows(page).filter({ hasText: subject }).first();
  // Scrolling renders and measures further rows, which can move the row
  // again: repeat until the position holds (twice at most in practice).
  for (let pass = 0; pass < 5; pass++) {
    await listScrollSettled(page);
    const moved = await row.evaluate((element) => {
      const list = element.closest('[data-part="message-list"]');
      const spacer = list?.querySelector('[data-part="message-list.spacer"]');
      if (!(list instanceof HTMLElement) || !spacer) {
        throw new Error("the row is not in a message list");
      }
      // The virtualizer's own rule for `align: "auto"` from the top: the
      // row's end (measured from the first row's spacer) at the bottom edge.
      const end = element.getBoundingClientRect().bottom - spacer.getBoundingClientRect().top;
      const target = Math.max(0, Math.round(end - list.clientHeight));
      const before = list.scrollTop;
      list.scrollTop = target;
      return Math.abs(list.scrollTop - before) >= 1;
    });
    if (!moved) break;
  }
  await expect(row).toBeInViewport();
}

async function selectSample(page: Page) {
  await selectMessage(page, SAMPLE_MESSAGE);
}

export const SCREENS: readonly Screen[] = [
  {
    id: "main",
    title: "Main window with a message open",
    async open(page) {
      await selectSample(page);
    },
  },
  {
    id: "composer",
    title: "Composer",
    async open(page) {
      await page.locator('[data-part="folder-tree.compose"]').click();
      // M5c: every message has its own window `composer:<key>`.
      const composer = page.locator('[data-part="window"][data-window^="composer:"]');
      await expect(composer).toBeVisible();
      await composer.getByRole("textbox", { name: "To" }).fill("grace@navy.example");
      await composer.getByRole("textbox", { name: "Subject" }).fill("Quarterly numbers");
      await composer
        .getByRole("textbox", { name: "Message" })
        .fill("Hi Grace,\n\nthe numbers for the third quarter are attached.\n\nAda");
      // M5c: the security panel checks this composer's own recipients
      // (debounced); capture after its answer, not before.
      await expect(
        composer.getByRole("button", { name: "Look up the missing key online" }),
      ).toBeVisible();
    },
  },
  {
    id: "settings",
    title: "Settings",
    async open(page) {
      const settings = await showView(page, "Settings");
      // The page remembers its last section (UI state, shared by the
      // variants of one worker): the appearance screen leaves Appearance.
      await settings
        .getByRole("navigation", { name: "Settings sections", exact: true })
        .getByRole("button", { name: "Accounts", exact: true })
        .click();
      await expect(settings.locator('[data-part="settings-page.title"]')).toHaveText("Accounts");
    },
  },
  {
    id: "calendar",
    title: "Calendar (week of the fixed events)",
    // The app reads "today" when it loads, so the whole page starts at this time.
    time: CALENDAR_TIME,
    async open(page) {
      await calendarWeek(page);
    },
  },
  {
    id: "calendar-month",
    title: "Calendar, month view (cell lines, outside days, chips cut at the cell)",
    time: CALENDAR_TIME,
    async open(page) {
      const calendar = await calendarWeek(page);
      await chooseView(calendar, "Month", "month");
      await expect(calendar.locator('[data-part="calendar.month"]')).toBeVisible();
    },
  },
  {
    id: "calendar-year",
    title: "Calendar, year view (month names)",
    time: CALENDAR_TIME,
    async open(page) {
      const calendar = await calendarWeek(page);
      await chooseView(calendar, "Year", "year");
      await expect(calendar.locator('[data-part="calendar.year.month.title"]')).toHaveCount(12);
    },
  },
  {
    id: "calendar-agenda",
    title: "Calendar, agenda",
    time: CALENDAR_TIME,
    async open(page) {
      const calendar = await calendarWeek(page);
      await chooseView(calendar, "Agenda", "agenda");
      await expect(calendar.locator('[data-part="calendar.agenda"]')).toBeVisible();
    },
  },
  {
    id: "calendar-free-time",
    title: "Calendar, free time of the working days",
    time: CALENDAR_TIME,
    async open(page) {
      const calendar = await calendarWeek(page);
      await chooseView(calendar, "Free time", "free-time");
      await expect(calendar.locator('[data-part="calendar.free-time.day"]')).toHaveCount(5);
      await expect(calendar.locator('[data-part="calendar.free-time"]')).not.toHaveAttribute(
        "aria-busy",
        "true",
      );
    },
  },
  {
    id: "calendar-1024",
    title: "Calendar at 1024 px (the view menu replaces the segmented switch)",
    time: CALENDAR_TIME,
    viewport: NARROW,
    async open(page) {
      const calendar = await calendarWeek(page);
      await expect(calendar.getByRole("combobox", { name: "View", exact: true })).toBeVisible();
    },
  },
  {
    id: "contacts",
    title: "Contacts view (header, list, card)",
    async open(page) {
      const contacts = await showView(page, "Contacts");
      await expect(contacts.getByRole("heading", { name: "Contacts", exact: true })).toBeVisible();
      await expect(contacts.locator('[data-part="contact-list.item"]').first()).toBeVisible();
    },
  },
  {
    id: "tasks",
    title: "Tasks view (header, filters, quick add)",
    async open(page) {
      const tasks = await showView(page, "Tasks");
      await expect(tasks.getByRole("heading", { name: "Tasks", exact: true })).toBeVisible();
    },
  },
  {
    id: "main-1024",
    title: "Main window at 1024 px with a message open",
    viewport: NARROW,
    async open(page) {
      await selectSample(page);
    },
  },
  {
    id: "more",
    title: "The reader's action bar with its More menu open (M5b)",
    async open(page) {
      await selectSample(page);
      await page
        .getByRole("toolbar", { name: "Message actions" })
        .getByRole("button", { name: "More", exact: true })
        .click();
      await expect(page.getByRole("menu")).toBeVisible();
    },
  },
  {
    id: "menu",
    title: "Context menu of a message",
    async open(page) {
      await selectSample(page);
      await messageRows(page)
        .filter({ hasText: SAMPLE_MESSAGE })
        .first()
        .click({ button: "right" });
      await expect(page.getByRole("menu")).toBeVisible();
    },
  },
  {
    id: "html",
    title: "An HTML newsletter with its remote content blocked (reader column and banner)",
    async open(page) {
      await selectMessage(page, HTML_MESSAGE);
      await expect(page.locator('[data-part="remote-content"]')).toBeVisible();
      await expect(
        page.frameLocator('iframe[data-part="message-view.body"]').locator("body"),
      ).not.toBeEmpty();
    },
  },
  {
    id: "attachment",
    title: "A message with attachment cards",
    async open(page) {
      await selectMessage(page, ATTACHMENT_MESSAGE);
      await expect(page.locator('[data-part="attachment-card"]')).toHaveCount(2);
    },
  },
  {
    id: "attachment-saved",
    title: "The attachment cards after saving one (the status keeps the card height)",
    async open(page) {
      await selectMessage(page, ATTACHMENT_MESSAGE);
      const card = page.locator('[data-part="attachment-card"]', { hasText: "usage.csv" });
      const download = page.waitForEvent("download");
      await card.getByRole("button", { name: "Save usage.csv" }).click();
      await download;
      await expect(card.getByRole("status")).toBeVisible();
    },
  },
  {
    id: "dialog",
    title: "Dialog (Move to…)",
    async open(page) {
      await selectSample(page);
      await messageRows(page)
        .filter({ hasText: SAMPLE_MESSAGE })
        .first()
        .click({ button: "right" });
      await page.getByRole("menu").getByRole("menuitem", { name: "Move to…" }).click();
      await expect(layoutWindow(page, "move")).toBeVisible();
    },
  },
  {
    id: "confirmation",
    title: "Modal confirmation (Delete permanently?)",
    // P3 (ADR 0164): it asks only in Trash / Junk, with the message count;
    // the sample conversation goes to Trash first and comes back afterwards.
    async open(page) {
      await selectSample(page);
      const row = messageRows(page).filter({ hasText: SAMPLE_MESSAGE }).first();
      const account = (await row.getAttribute("data-account")) ?? "";
      await page.keyboard.press("Delete");
      await expect(messageRows(page).filter({ hasText: SAMPLE_MESSAGE })).toHaveCount(0);
      // The undo toast times out; the screen shows without it.
      await page
        .locator('[data-part="mail-actions.toast"]')
        .getByRole("button", { name: "Dismiss" })
        .click();
      await page
        .locator(`[data-part="folder-tree.item"][data-role="trash"][data-account="${account}"]`)
        .first()
        .click();
      await selectMessage(page, SAMPLE_MESSAGE);
      await page.keyboard.press("Shift+Delete");
      const confirm = layoutWindow(page, "mail-delete-permanently");
      await expect(confirm).toBeVisible();
      await expect(page.locator('[data-part="window-scrim"]')).toBeVisible();
    },
    async close(page) {
      const confirm = layoutWindow(page, "mail-delete-permanently");
      await confirm.getByRole("button", { name: "Cancel" }).click();
      await expect(confirm).toBeHidden();
      await messageRows(page).filter({ hasText: SAMPLE_MESSAGE }).first().click();
      await page.keyboard.press("z");
      await expect(messageRows(page).filter({ hasText: SAMPLE_MESSAGE })).toHaveCount(0);
    },
  },
  // M5d: the in-app editors (nothing is written: no screen changes a file).
  {
    id: "layout-editor",
    title: "Customize layout, a pane picked up (drop targets, presets, palette)",
    // No message selected: the list's header row would scroll it by a
    // timing-dependent amount to keep the selected row visible.
    async open(page) {
      await runFromPalette(page, "customize layout", "Customize layout");
      const editor = page.getByRole("region", { name: "Customizing layout: Mail" });
      await expect(editor).toBeVisible();
      await editor.getByRole("button", { name: "Move Folders", exact: true }).click();
      await expect(
        page.getByRole("button", { name: "Swap Folders with Message", exact: true }),
      ).toBeVisible();
    },
  },
  {
    id: "toolbar-customizer",
    title: "Customize toolbar (bar, More menu, available actions)",
    async open(page) {
      await selectSample(page);
      await page
        .getByRole("toolbar", { name: "Message actions" })
        .getByRole("button", { name: "Reply", exact: true })
        .click({ button: "right" });
      await page.getByRole("menu").getByRole("menuitem", { name: "Customize toolbar" }).click();
      const customizer = page.locator('[data-part="window"][data-window^="toolbar-customizer"]');
      await expect(customizer.getByRole("listbox", { name: "Available actions" })).toBeVisible();
      await expect(customizer.getByRole("option", { name: /^Reply all/ })).toBeVisible();
    },
  },
  {
    id: "appearance",
    title: "Settings → Appearance (gallery, New theme / Edit theme, density, sliders)",
    async open(page) {
      const settings = await showView(page, "Settings");
      await settings
        .getByRole("navigation", { name: "Settings sections", exact: true })
        .getByRole("button", { name: "Appearance", exact: true })
        .click();
      await expect(settings.getByRole("radiogroup", { name: "Themes" })).toBeVisible();
    },
  },
  {
    id: "conversation",
    title: "A long conversation: its row in the list and the reader's card feed (P2)",
    async open(page) {
      await selectConversation(page);
    },
  },
  {
    id: "conversation-expanded",
    title: "The long conversation with every card open (Expand all, P2)",
    async open(page) {
      const feed = await selectConversation(page);
      await page.getByRole("button", { name: "Expand all" }).click();
      await expect(
        feed.getByRole("article").and(page.locator('[data-state~="expanded"]')),
      ).toHaveCount(12);
    },
  },
  {
    id: "theme-editor",
    title: "Theme editor on the theme in use (tokens, live preview)",
    async open(page) {
      await runFromPalette(page, "edit theme", "Edit theme");
      const editor = layoutWindow(page, "theme-editor");
      await expect(editor.getByRole("tablist", { name: "Theme sections" })).toBeVisible();
      await expect(
        editor.getByRole("textbox", { name: "Value of color.accent", exact: true }),
      ).toBeVisible();
    },
  },
  // ADR 0190: the connection log (its lines carry times and durations, so
  // the list is masked) and the details of a folder the server refused.
  {
    id: "connection-log",
    title: "The connection log of the IMAP account (Settings → Accounts → Connection log)",
    async open(page) {
      await openWorkLog(page);
      await expect(
        workLogWindow(page).locator('[data-part="connection-log.entry"]').first(),
      ).toBeVisible();
    },
    mask: (page) => [
      workLogWindow(page).locator('[data-part="connection-log.list"]'),
      workLogWindow(page).locator('[data-part="connection-log.status"]'),
    ],
  },
  {
    id: "error-details",
    title: "Show details of a folder the server refused to open",
    async open(page, server) {
      await showRefusedFolderDetails(page, server, await otherMailbox(server));
    },
    async close(page, server) {
      await errorDetailsWindow(page).locator('[data-part="window.close"]').click();
      await allowAllFolders(page, server);
    },
    // The rows are `display: contents`: mask their values (time, command tag).
    mask: (page) =>
      ["at", "command", "server"].map((field) =>
        errorDetailsWindow(page).locator(
          `[data-field="${field}"] [data-part="error-details.value"]`,
        ),
      ),
  },
];

/** The iCalendar object of a fixed calendar event. */
export function calendarEventIcs(event: (typeof CALENDAR_EVENTS)[number]): string {
  return [
    "BEGIN:VCALENDAR",
    "VERSION:2.0",
    "PRODID:-//Nail visual tests//EN",
    "BEGIN:VEVENT",
    `UID:${event.uid}@nail.example`,
    "DTSTAMP:20260101T000000Z",
    `DTSTART:${event.start}`,
    `DTEND:${event.end}`,
    `SUMMARY:${event.title}`,
    "END:VEVENT",
    "END:VCALENDAR",
    "",
  ].join("\r\n");
}
