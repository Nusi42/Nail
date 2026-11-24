import { expect, test, type Locator, type Page } from "@playwright/test";
import { eventChip, icalTime, icsEvent, openCalendar, syncCalendars } from "../support/calendar";
import { WORK } from "../support/invitations";
import { control, nailPerTest } from "../support/nail";
import { saveSettings } from "../support/settings";
import { openMail, openSettings } from "../support/views";

/**
 * Wave R7 (ADR 0246) against `nail serve --seed demo-multi`, the behaviours
 * the first specs left out: tasks meeting the calendar (the "Due this week"
 * panel, due tasks with a checkbox in the all-day row, planning a task from
 * the keyboard and dragging one onto the grid), hiding declined
 * invitations, the second time zone, the year view's heat, the empty
 * state, and the quick look staying inside the window once its details
 * load.
 */

const server = nailPerTest({ seed: "demo-multi" });

async function open(page: Page, size = { width: 1400, height: 900 }): Promise<Locator> {
  await openMail(page, server.url, { size });
  const calendar = await openCalendar(page);
  await calendar.getByRole("button", { name: "Week", exact: true }).click();
  return calendar;
}

/** Adds a task due today from the "Due this week" panel; answers the panel. */
async function addDueTask(calendar: Locator, title: string): Promise<Locator> {
  const panel = calendar.getByRole("region", { name: "Due this week" });
  await panel.getByRole("textbox", { name: /^Add a task due today to / }).fill(title);
  await panel.getByRole("button", { name: "Add", exact: true }).click();
  await expect(panel.getByText(title, { exact: true })).toBeVisible();
  return panel;
}

/** The task's entry in the time grid's all-day row. */
const allDayTask = (calendar: Locator, title: string) =>
  calendar.locator('[data-part="calendar.task"]').filter({ hasText: title });

test("a task due today shows in the panel and the all-day row, is planned from the keyboard and completed there", async ({
  page,
}) => {
  const calendar = await open(page);
  const panel = await addDueTask(calendar, "Write the report");
  await expect(allDayTask(calendar, "Write the report")).toBeVisible();

  // Choose a slot with the keyboard, then plan the task there: no drag needed.
  await calendar.locator('[data-part="calendar.slot"][tabindex="0"]').focus();
  await page.keyboard.press("ArrowDown");
  const plan = panel.getByRole("button", { name: "Plan “Write the report” in the calendar" });
  await plan.focus();
  await page.keyboard.press("Enter");
  await expect(eventChip(calendar, "Write the report")).toBeVisible();
  // The task stays open.
  await expect(panel.getByText("Write the report", { exact: true })).toBeVisible();

  // Completing it in the all-day row takes it off the panel and the row.
  await allDayTask(calendar, "Write the report").getByRole("checkbox").click();
  await expect(allDayTask(calendar, "Write the report")).toHaveCount(0);
  await expect(panel.getByText("Write the report", { exact: true })).toHaveCount(0);
});

test("a task dragged from the panel onto the grid blocks time with its title", async ({ page }) => {
  const calendar = await open(page);
  const panel = await addDueTask(calendar, "Call the bank");
  const item = panel.locator('[data-part="due-tasks.item"]').filter({ hasText: "Call the bank" });
  const slots = calendar.locator('[data-part="calendar.slots"]');
  const box = await calendar.locator('[data-part="calendar.scroll"]').boundingBox();
  const area = await slots.boundingBox();
  if (!box || !area) throw new Error("no time grid");
  // The middle of the visible part of the grid, in the first day column.
  await item.dragTo(slots, {
    targetPosition: { x: area.width / 14, y: box.y - area.y + box.height / 2 },
  });
  await expect(eventChip(calendar, "Call the bank")).toBeVisible();
  await expect(panel.getByText("Call the bank", { exact: true })).toBeVisible();
});

test("declined invitations are struck through, or hidden with the preference off", async ({
  page,
}) => {
  await control(server, "dav-put", {
    collection: "personal",
    name: "declined.ics",
    data: icsEvent("e2e-r7-declined@example.org", "Vendor pitch", icalTime(0, 6), icalTime(0, 7), [
      "ORGANIZER;CN=Grace Hopper:mailto:grace@navy.example",
      "ATTENDEE;CN=Grace Hopper;PARTSTAT=ACCEPTED:mailto:grace@navy.example",
      `ATTENDEE;CN=Ada Lovelace;PARTSTAT=DECLINED:mailto:${WORK}`,
    ]),
  });
  const calendar = await open(page);
  await syncCalendars(page);
  const chip = eventChip(calendar, "Vendor pitch");
  await expect(chip).toHaveAttribute("data-state", /declined/);

  const settings = await openSettings(page, "Calendar & contacts");
  const card = settings.getByRole("region", { name: "Calendar preferences" });
  await card
    .getByRole("checkbox", { name: "Show declined invitations (struck through)" })
    .uncheck();
  await saveSettings(page);
  await expect(card.getByRole("status")).toHaveText("Calendar preferences saved.");
  const again = await openCalendar(page);
  await expect(again.locator('[data-part="calendar.time-grid"]')).toBeVisible();
  await expect(eventChip(again, "Vendor pitch")).toHaveCount(0);
});

test("a second time zone from the zone chip adds a gutter to the time grid", async ({ page }) => {
  const calendar = await open(page);
  await calendar.locator('[data-part="calendar.zone-chip"]').click();
  const popover = page.getByRole("dialog", { name: "Time zones" });
  await popover.getByRole("combobox", { name: "Second time zone" }).selectOption("Asia/Tokyo");
  await expect(
    calendar.locator('[data-part="calendar.zone"][data-zone="secondary"]'),
  ).toBeVisible();
  await expect(
    calendar.getByRole("button", { name: /^Time zones: .* and Asia\/Tokyo/ }),
  ).toBeVisible();
});

test("the year shades busy days, and an empty range offers the way back", async ({ page }) => {
  const calendar = await open(page);
  await calendar.getByRole("button", { name: "Year", exact: true }).click();
  await expect(
    calendar
      .locator('[data-part="calendar.year.grid.day"][data-heat]:not([data-heat="0"])')
      .first(),
  ).toBeVisible();

  // With every calendar hidden, the agenda says so and offers today and a new event.
  await calendar.getByRole("button", { name: "Agenda", exact: true }).click();
  const toggles = calendar.locator('[data-part="calendar-list.toggle"]');
  for (const toggle of await toggles.all()) {
    await toggle.click();
    await expect(toggle).not.toBeChecked();
  }
  const empty = calendar.locator('[data-part="pim.empty"]');
  await expect(empty).toContainText(/Nothing planned/);
  await expect(empty.getByRole("button", { name: "Back to today" })).toBeVisible();
  await expect(empty.getByRole("button", { name: "New event" })).toBeVisible();
});

test("the quick look stays inside the window once its details load", async ({ page }) => {
  await openMail(page, server.url, { size: { width: 1400, height: 620 } });
  const calendar = await openCalendar(page, { agenda: true });
  // Project kickoff has people and answers: the quick look grows when they load.
  const chip = eventChip(calendar, "Project kickoff");
  await chip.scrollIntoViewIfNeeded();
  await chip.click();
  const look = page.getByRole("dialog", { name: "Project kickoff" });
  await expect(look.getByText(/^2 people/)).toBeVisible();
  await expect
    .poll(async () => {
      const box = await look.boundingBox();
      return box ? box.y + box.height : Number.POSITIVE_INFINITY;
    })
    .toBeLessThanOrEqual(620);
  await expect(look.getByRole("button", { name: "Edit" })).toBeInViewport();
});
