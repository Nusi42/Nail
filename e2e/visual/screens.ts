/**
 * The screens of the visual-regression harness (plan M5a WP154): the main
 * window with a message open, the composer, Settings, the calendar, a
 * context menu and a dialog. Each starts from a freshly loaded main window
 * (so one screen never leaks state into the next) and ends with the screen
 * open and its content present; `support.ts` then settles and captures it.
 *
 * Windows are found by their layout window name
 * (`[data-part="window"][data-window=…]`), controls by role and accessible
 * name, as in the regular e2e suite. When the shell changes (M5b: the
 * navigation rail and full-page views), the `open` functions here are the
 * only place that needs to follow.
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

export const mainToolbar = (page: Page) => page.getByRole("toolbar", { name: "Main toolbar" });
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

async function selectSample(page: Page) {
  await rows(page).filter({ hasText: SAMPLE_MESSAGE }).first().click();
  await expect(page.locator('[data-part="message-view.header.subject"]')).toContainText(
    SAMPLE_MESSAGE,
  );
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
      await mainToolbar(page).getByRole("button", { name: "New message" }).click();
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
      await mainToolbar(page).getByRole("button", { name: "Settings" }).click();
      await expect(page.getByRole("dialog", { name: "Settings" })).toBeVisible();
    },
  },
  {
    id: "calendar",
    title: "Calendar (week of the fixed events)",
    // The app reads "today" when it loads, so the whole page starts at this time.
    time: CALENDAR_TIME,
    async open(page) {
      await mainToolbar(page).getByRole("button", { name: "Calendar", exact: true }).click();
      const calendar = layoutWindow(page, "calendar");
      await expect(calendar).toBeVisible();
      for (const event of CALENDAR_EVENTS) {
        await expect(
          calendar.getByRole("button", { name: new RegExp(`^${event.title},`) }).first(),
        ).toBeVisible();
      }
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
