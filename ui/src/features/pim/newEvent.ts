/**
 * Starting a new event or task from the views ("new event at 10:00 on
 * Tuesday", "new all-day event", "create event from email"): fills the
 * editor's target and opens it, or (R7 M5, S12) saves an event with only a
 * title and its times straight away — the time grid's inline title after
 * drawing a range, a task dropped on the grid.
 */
import type { CalendarDate } from "@internationalized/date";
import type {
  Calendar,
  CalendarId,
  EventFields,
  PimTime,
  Recurrence,
  TaskFields,
} from "../../api/generated";
import { DEFAULT_EVENT_MINUTES, MINUTES_PER_DAY } from "../../api/generated";
import type { ChangeContext } from "./changes";
import { emptyEventFields } from "./defaults";
import { EVENT_DIALOG, TASK_DIALOG } from "./names";
import { saveEvent, type PimWriteContext } from "./queries";
import { dateString, makePimTime, type MinuteRange } from "./time";

/** Length of a new event, minutes (the core's `DEFAULT_EVENT_MINUTES`). */
export const NEW_EVENT_MINUTES = DEFAULT_EVENT_MINUTES;

/**
 * The times of `range` (minutes after midnight of `day`; the end may pass
 * midnight) written in `tz`.
 */
export function rangeTimes(
  day: CalendarDate,
  range: MinuteRange,
  tz: string,
): { start: PimTime; end: PimTime } {
  const at = (minutes: number) => {
    const date = day.add({ days: Math.floor(minutes / MINUTES_PER_DAY) });
    const rest = ((minutes % MINUTES_PER_DAY) + MINUTES_PER_DAY) % MINUTES_PER_DAY;
    return makePimTime(date, { hour: Math.floor(rest / 60), minute: rest % 60 }, tz);
  };
  return { start: at(range.start), end: at(range.end) };
}

export function openNewEvent(
  ctx: Pick<ChangeContext, "store" | "pim">,
  day: CalendarDate,
  time: { hour: number; minute: number } | null,
  tz: string,
  calendarId: CalendarId | null = null,
  fields: EventFields | null = null,
  recurrence: Recurrence | null = null,
): void {
  const startMinute = time === null ? 0 : time.hour * 60 + time.minute;
  const times =
    time === null
      ? {
          start: { type: "date" as const, date: dateString(day) },
          end: { type: "date" as const, date: dateString(day.add({ days: 1 })) },
        }
      : rangeTimes(day, { start: startMinute, end: startMinute + NEW_EVENT_MINUTES }, tz);
  openNewEventWith(
    ctx,
    fields?.start ?? times.start,
    fields?.end ?? times.end,
    calendarId,
    fields,
    recurrence,
  );
}

/**
 * Opens the editor on a new event over `range` of `day` in `calendar` (R7
 * M5: "More options" of the inline title), keeping what was typed and the
 * calendar's default reminders.
 */
export function openNewEventIn(
  ctx: Pick<ChangeContext, "store" | "pim">,
  day: CalendarDate,
  range: MinuteRange,
  tz: string,
  calendar: Calendar | null,
  title = "",
): void {
  const { start, end } = rangeTimes(day, range, tz);
  openNewEventWith(
    ctx,
    start,
    end,
    calendar?.id ?? null,
    newEventFields(start, end, calendar, title),
  );
}

/** The fields of a new event with only a title and times (`calendar`'s default reminders). */
function newEventFields(
  start: PimTime,
  end: PimTime,
  calendar: Calendar | null,
  title: string,
): EventFields {
  return {
    ...emptyEventFields(start, end),
    title,
    alarms: [...(calendar?.settings.default_alarms ?? [])],
  };
}

function openNewEventWith(
  ctx: Pick<ChangeContext, "store" | "pim">,
  start: PimTime,
  end: PimTime,
  calendarId: CalendarId | null,
  fields: EventFields | null,
  recurrence: Recurrence | null = null,
): void {
  ctx.pim.getState().editEvent({ mode: "new", start, end, calendarId, fields, recurrence });
  ctx.store.getState().openWindow(EVENT_DIALOG);
}

/** The calendars a new event can go to: writable ones that hold events. */
export function writableEventCalendars(calendars: readonly Calendar[]): Calendar[] {
  return calendars.filter((c) => !c.read_only && c.components.includes("event"));
}

/** The task lists a new task can go to (writable, holding tasks). */
export function writableTaskLists(calendars: readonly Calendar[]): Calendar[] {
  return calendars.filter((c) => !c.read_only && c.components.includes("task"));
}

/**
 * The calendar a new event goes to without asking: `preferred` when it is
 * writable, else the first local one, else the first writable one.
 */
export function defaultEventCalendar(
  calendars: readonly Calendar[],
  preferred: CalendarId | null = null,
): Calendar | null {
  const writable = writableEventCalendars(calendars);
  return (
    writable.find((c) => c.id === preferred) ??
    writable.find((c) => c.source_id === "local") ??
    writable[0] ??
    null
  );
}

/**
 * Saves a new event with `title` over `range` of `day` in `calendar` (with
 * its default reminders); the write goes through the core's operation
 * queue like every other.
 */
export async function createEventIn(
  ctx: PimWriteContext,
  calendar: Calendar,
  day: CalendarDate,
  range: MinuteRange,
  tz: string,
  title: string,
): Promise<void> {
  const { start, end } = rangeTimes(day, range, tz);
  await saveEvent(
    ctx,
    { mode: "create", calendarId: calendar.id },
    newEventFields(start, end, calendar, title),
    null,
  );
}

export function openNewTask(
  ctx: Pick<ChangeContext, "store" | "pim">,
  listId: CalendarId | null,
  parentId: string | null = null,
  fields: TaskFields | null = null,
): void {
  ctx.pim.getState().editTask({ mode: "new", listId, parentId, fields });
  ctx.store.getState().openWindow(TASK_DIALOG);
}
