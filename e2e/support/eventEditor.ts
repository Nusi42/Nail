/**
 * The event editor in end-to-end tests (R7, DRY): a new event from the
 * calendar's agenda with its title typed in.
 */
import type { Locator, Page } from "@playwright/test";
import { openCalendar } from "./calendar";

/** Shows the agenda, opens "New event" and types `title`; the calendar and the editor. */
export async function startNewEvent(
  page: Page,
  title: string,
): Promise<{ calendar: Locator; editor: Locator }> {
  const calendar = await openCalendar(page, { agenda: true });
  await calendar.getByRole("button", { name: "New event" }).click();
  const editor = page.getByRole("dialog", { name: "Event", exact: true });
  await editor.getByLabel("Title", { exact: true }).fill(title);
  return { calendar, editor };
}
