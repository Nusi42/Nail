/**
 * The screens of the visual-regression harness (plan M5a WP154, M5b): the
 * main window (rail + mail view) with a message open, the reader's "More"
 * menu, the composer, the settings page, the calendar view, a context menu,
 * HTML and attachment messages and a dialog. Each starts from a freshly
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

/** The fixed clock of every screen but the calendar: after the demo mail's dates. */
export const MAIN_TIME = new Date("2026-09-30T10:00:00Z");

/**
 * The calendar's fixed clock: a week long before any date the demo data
 * derives from the real "today" (its feeds and meetings start at the real
 * date), so only the fixed events of {@link CALENDAR_EVENTS} show.
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
  /** Opens the screen on a freshly loaded main window. */
  open(page: Page): Promise<void>;
  /** Parts that legitimately differ between runs (none so far). */
  mask?(page: Page): Locator[];
}

/** The navigation rail (M5b). */
export const rail = (page: Page) => page.getByRole("navigation", { name: "Sections", exact: true });

/** Shows a view from the rail; returns its region. */
async function showView(page: Page, name: "Calendar" | "Settings"): Promise<Locator> {
  await rail(page)
    .getByRole("button", { name: new RegExp(`^${name}(,|$)`) })
    .click();
  const region = page.getByRole("region", { name: `${name} view`, exact: true });
  await expect(region).toBeVisible();
  return region;
}
const rows = (page: Page) => page.locator('[data-part="message-list.row"]');
export const layoutWindow = (page: Page, name: string) =>
  page.locator(`[data-part="window"][data-window="${name}"]`);

/** Loads the app and waits until the demo mail is listed. */
export async function loadMainWindow(page: Page, url: string): Promise<void> {
  await page.goto(url);
  await expect(rows(page).filter({ hasText: SAMPLE_MESSAGE }).first()).toBeVisible();
  // Account states show (status dots); never capture one mid-connect.
  await expect(page.locator('[data-status="connecting"]')).toHaveCount(0, { timeout: 60_000 });
}

/** A demo newsletter with an HTML body and blocked remote images. */
export const HTML_MESSAGE = "Your weekly garden digest";
/** The demo invoice with a PDF and a CSV attachment. */
export const ATTACHMENT_MESSAGE = "Your invoice for September 2026";

async function selectMessage(page: Page, subject: string) {
  await rows(page).filter({ hasText: subject }).first().click();
  await expect(page.locator('[data-part="message-view.header.subject"]')).toContainText(subject);
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
      const composer = layoutWindow(page, "composer");
      await expect(composer).toBeVisible();
      await composer.getByRole("textbox", { name: "To" }).fill("grace@navy.example");
      await composer.getByRole("textbox", { name: "Subject" }).fill("Quarterly numbers");
      await composer
        .getByRole("textbox", { name: "Message" })
        .fill("Hi Grace,\n\nthe numbers for the third quarter are attached.\n\nAda");
    },
  },
  {
    id: "settings",
    title: "Settings",
    async open(page) {
      const settings = await showView(page, "Settings");
      await expect(settings.locator('[data-part="settings-page.title"]')).toHaveText("Accounts");
    },
  },
  {
    id: "calendar",
    title: "Calendar (week of the fixed events)",
    // The app reads "today" when it loads, so the whole page starts at this time.
    time: CALENDAR_TIME,
    async open(page) {
      const calendar = await showView(page, "Calendar");
      for (const event of CALENDAR_EVENTS) {
        await expect(
          calendar.getByRole("button", { name: new RegExp(`^${event.title},`) }).first(),
        ).toBeVisible();
      }
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
      await rows(page).filter({ hasText: SAMPLE_MESSAGE }).first().click({ button: "right" });
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
      await rows(page).filter({ hasText: SAMPLE_MESSAGE }).first().click({ button: "right" });
      await page.getByRole("menu").getByRole("menuitem", { name: "Move to…" }).click();
      await expect(layoutWindow(page, "move")).toBeVisible();
    },
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
