/**
 * The calendar in end-to-end tests (wave Q2, DRY): the view, its event
 * chips and iCalendar times for events the mock servers serve.
 */
import { expect, type Locator, type Page } from "@playwright/test";
import { showView, toolbarAction, viewRegion } from "./views";

/** Shows the calendar view (in its agenda with `agenda`); answers the view. */
export async function openCalendar(
  page: Page,
  options: { agenda?: boolean } = {},
): Promise<Locator> {
  await showView(page, "Calendar");
  const calendar = viewRegion(page, "Calendar");
  await expect(calendar).toBeVisible();
  if (options.agenda === true) {
    await calendar.getByRole("button", { name: "Agenda", exact: true }).click();
  }
  return calendar;
}

/** Syncs every source (and Exchange account) from the calendar's toolbar. */
export async function syncCalendars(page: Page): Promise<void> {
  await toolbarAction(
    page,
    page.getByRole("toolbar", { name: "Calendar actions" }),
    "Sync calendars and contacts",
  );
}

/** An event chip in the calendar views, by the start of its label. */
export const eventChip = (scope: Locator, title: string) =>
  scope.getByRole("button", { name: new RegExp(`^${title},`) }).first();

/** Today + `days` at `hour`:00 UTC, as an iCalendar UTC time. */
export function icalTime(days: number, hour: number): string {
  const d = new Date();
  d.setUTCDate(d.getUTCDate() + days);
  d.setUTCHours(hour, 0, 0, 0);
  return d
    .toISOString()
    .replace(/[-:]/g, "")
    .replace(/\.\d{3}/, "");
}
