import { expect, test, type Locator, type Page } from "@playwright/test";
import { openCalendar } from "../support/calendar";
import { nailOnDemand } from "../support/nail";
import { saveSettings } from "../support/settings";
import { openMail, openSettings } from "../support/views";

/**
 * Wave R7, views and shell (ADR 0246) against `nail serve --seed
 * demo-multi`: narrow windows (the sidebar as a drawer, a floating New
 * event, never a day column narrower than about 90 px), quick add from
 * text, the calendar search, the month view's bars and lines, the agenda's
 * time column and the calendar preferences saved in Settings.
 */

const server = nailOnDemand();

async function open(page: Page, size = { width: 1400, height: 900 }) {
  await server.start({ seed: "demo-multi" });
  await openMail(page, server.url, { size });
}

/** The day heads of the time grid. */
const dayHeads = (calendar: Locator) => calendar.locator('[data-part="calendar.day-head"]');

/** The page never scrolls sideways. */
async function expectNoSidewaysScroll(page: Page) {
  const overflow = await page.evaluate(() => {
    const root = document.scrollingElement ?? document.documentElement;
    return root.scrollWidth - root.clientWidth;
  });
  expect(overflow).toBeLessThanOrEqual(1);
}

/** The narrowest day column of the time grid, in CSS pixels. */
async function narrowestDay(calendar: Locator): Promise<number> {
  const widths = await dayHeads(calendar).evaluateAll((els) =>
    els.map((el) => el.getBoundingClientRect().width),
  );
  return Math.min(...widths);
}

test("at 420 px the calendar shows few wide days, a drawer and a floating New event", async ({
  page,
}) => {
  await open(page, { width: 420, height: 800 });
  const calendar = await openCalendar(page);
  await expect(dayHeads(calendar).first()).toBeVisible();
  expect([1, 3]).toContain(await dayHeads(calendar).count());
  expect(await narrowestDay(calendar)).toBeGreaterThanOrEqual(88);
  await expectNoSidewaysScroll(page);

  // One New event, floating over the view.
  const fab = calendar.getByRole("button", { name: "New event", exact: true });
  await expect(fab).toHaveCount(1);
  await expect(fab).toHaveAttribute("data-part", "calendar.fab");

  // The sidebar is a drawer: mini-month, calendars, tasks due this week.
  const toggle = calendar.getByRole("button", { name: "Show calendars", exact: true });
  await toggle.click();
  const drawer = calendar.getByRole("complementary", { name: "Calendars", exact: true });
  await expect(drawer).toBeVisible();
  await expect(drawer.locator('[data-part="mini-month"]')).toBeVisible();
  await expect(drawer.getByRole("region", { name: "Due this week" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(drawer).toBeHidden();
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
});

test("at 760 px the sidebar is a drawer and no day column gets narrower than about 90 px", async ({
  page,
}) => {
  await open(page, { width: 760, height: 800 });
  const calendar = await openCalendar(page);
  await expect(dayHeads(calendar).first()).toBeVisible();
  await expect(calendar.getByRole("button", { name: "Show calendars", exact: true })).toBeVisible();
  expect(await narrowestDay(calendar)).toBeGreaterThanOrEqual(88);
  await expectNoSidewaysScroll(page);

  // Wide again: the sidebar is back, the drawer button and the floating button go.
  await page.setViewportSize({ width: 1400, height: 900 });
  await expect(calendar.locator('[data-part="mini-month"]')).toBeVisible();
  await expect(calendar.getByRole("button", { name: "Show calendars", exact: true })).toHaveCount(
    0,
  );
  await expect(dayHeads(calendar)).toHaveCount(7);
});

test("quick add previews the text as chips and adds the event with Enter", async ({ page }) => {
  await open(page);
  const calendar = await openCalendar(page);
  const field = calendar.getByRole("textbox", { name: "Quick add an event" });
  await field.fill("Lunch with Bob tomorrow 12:30 at Corner café");
  const preview = calendar.getByRole("list", { name: "What Nail understood" });
  await expect(preview.locator('[data-part="calendar.quick-add.chip"]').first()).toBeVisible();
  await expect(preview).toContainText("Lunch with Bob");
  await field.press("Enter");
  await expect(field).toHaveValue("");
  await expect(calendar.getByText(/^Added “Lunch with Bob/)).toBeVisible();
  // The calendar shows the event's day.
  await expect(calendar.getByRole("button", { name: /^Lunch with Bob/ }).first()).toBeVisible();

  // Tab opens the editor with the text; nothing is saved.
  await field.fill("Dentist check");
  await field.press("Tab");
  // The event editor opens with the text as its title.
  const editor = page.getByRole("dialog", { name: "Event", exact: true });
  await expect(editor).toBeVisible();
  await expect(editor.getByLabel("Title", { exact: true })).toHaveValue("Dentist check");
});

test("the calendar search finds an event, groups it and shows its day", async ({ page }) => {
  await open(page);
  const calendar = await openCalendar(page);
  // `/` focuses the field, as in Mail.
  await calendar.getByRole("button", { name: "Today", exact: true }).focus();
  await page.keyboard.press("/");
  const field = calendar.getByRole("combobox", { name: "Search events" });
  await expect(field).toBeFocused();
  await field.fill("kickoff");
  const results = calendar.getByRole("listbox", { name: "Events found" });
  const upcoming = results.getByRole("group", { name: "Upcoming" });
  await expect(upcoming.getByRole("option", { name: /Project kickoff/ })).toBeVisible();
  await field.press("Enter");
  await expect(calendar.locator('[data-part="calendar"]')).toHaveAttribute("data-view", "day");
  await expect(calendar.getByRole("button", { name: /^Project kickoff,/ }).first()).toBeVisible();
});

test("the month view draws all-day events as bars, timed ones as lines and today as a pill", async ({
  page,
}) => {
  await open(page);
  const calendar = await openCalendar(page);
  await calendar.getByRole("button", { name: "Month", exact: true }).click();
  const month = calendar.locator('[data-part="calendar.month"]');
  await expect(month.locator('[data-part="calendar.month.line"]').first()).toBeVisible();
  const bar = month
    .locator('[data-part="calendar.month.bar"]')
    .filter({ hasText: "Founders' Day" });
  await expect(bar.first()).toBeVisible();
  // The timed lines start with a colour dot and the time.
  const line = month.locator('[data-part="calendar.month.line"]').first();
  const dot = await line.evaluate((el) => getComputedStyle(el, "::before").borderLeftWidth);
  expect(dot).toBe("6px");
  // Today: a filled date, no frame around the cell.
  const today = month.locator('td[aria-current="date"]');
  expect(await today.evaluate((el) => getComputedStyle(el).boxShadow)).toBe("none");
  const pill = today.locator('[data-part="calendar.month.date"]');
  expect(await pill.evaluate((el) => getComputedStyle(el).backgroundColor)).not.toBe(
    "rgba(0, 0, 0, 0)",
  );
});

test("the agenda has a time column, Today / Tomorrow and a now marker", async ({ page }) => {
  await open(page);
  const calendar = await openCalendar(page, { agenda: true });
  const heads = calendar.locator('[data-part="calendar.agenda.day-head"]');
  await expect(heads.first()).toHaveText(/^Today, /);
  await expect(calendar.getByRole("heading", { name: /^Tomorrow, / })).toBeVisible();
  await expect(calendar.locator('[data-part="calendar.agenda.now"]')).toHaveCount(1);
  const review = calendar.getByRole("button", { name: /^Quarterly review,/ }).first();
  await expect(review.locator('[data-part="calendar.agenda.time"]')).toBeVisible();
  await expect(review.locator('[data-part="calendar.agenda.dot"]')).toBeVisible();
  // Day headers stay on top while the list scrolls.
  expect(await heads.first().evaluate((el) => getComputedStyle(el).position)).toBe("sticky");
});

test("calendar preferences save from Settings and show in the calendar", async ({ page }) => {
  await open(page);
  const settings = await openSettings(page, "Calendar & contacts");
  const card = settings.getByRole("region", { name: "Calendar preferences" });
  await card.getByRole("combobox", { name: "Working day starts" }).selectOption("540");
  await card.getByRole("checkbox", { name: "Show week numbers" }).check();
  await card.getByRole("checkbox", { name: "Saturday" }).check();
  await saveSettings(page);
  await expect(card.getByRole("status")).toHaveText("Calendar preferences saved.");
  const calendar = await openCalendar(page);
  const mini = calendar.locator('[data-part="mini-month"]');
  await expect(mini.getByRole("rowheader").first()).toHaveAccessibleName(/^Week \d+$/);
  // Saved in the core: shown again after a reload.
  await page.reload();
  const again = await openSettings(page, "Calendar & contacts");
  const reloaded = again.getByRole("region", { name: "Calendar preferences" });
  await expect(reloaded.getByRole("combobox", { name: "Working day starts" })).toHaveValue("540");
  await expect(reloaded.getByRole("checkbox", { name: "Show week numbers" })).toBeChecked();
});
