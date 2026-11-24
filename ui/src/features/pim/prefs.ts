/**
 * The calendar preferences and the R7 reading aids every calendar view
 * shares (contract, ADR 0246): working hours and days, the first day of
 * the week, ISO week numbers, the view zones, tasks due in a range, quick
 * create from text, and the drag payload of a task dropped on the time
 * grid. The core owns the preferences (`GetCalendarPreferences`); the
 * views only read them through these hooks, and the settings save them
 * with `saveCalendarPreferences`.
 *
 * Packages use these pieces and do not change them; a needed change goes
 * to the integrator.
 */
import { useQuery } from "@tanstack/react-query";
import type {
  CalendarId,
  CalendarPreferences,
  ParsedEventText,
  Task,
  Weekday,
} from "../../api/generated";
import { DEFAULT_CALENDAR_PREFERENCES, MINUTES_PER_DAY } from "../../api/generated";
import { DAY_MS } from "../../state/clock";
import type { NailClient } from "../../api/client";
import { useClient } from "../../state/queries";
import { pimWrite, type PimWriteContext } from "./queries";
import { WEEKDAYS, isKnownZone, localZone, type CalendarDate } from "./time";

/** Query keys of the R7 reads (under `pimKeys.all`, so PIM events refetch them). */
export const prefsKeys = {
  preferences: ["pim", "calendar-preferences"] as const,
  tasksDue: (from: string, to: string, lists: readonly string[] | null, tz: string) =>
    ["pim", "tasks-due", from, to, lists, tz] as const,
};

/** The calendar preferences; the defaults while they load or when the core cannot answer. */
export function useCalendarPreferences(): CalendarPreferences {
  const client = useClient();
  const query = useQuery({
    queryKey: prefsKeys.preferences,
    queryFn: async () =>
      (await client.request({ type: "GetCalendarPreferences" }, "CalendarPreferences")).preferences,
  });
  return query.data ?? DEFAULT_CALENDAR_PREFERENCES;
}

/** Saves the calendar preferences (the core checks them and announces the change). */
export async function saveCalendarPreferences(
  ctx: PimWriteContext,
  preferences: CalendarPreferences,
): Promise<CalendarPreferences> {
  return (
    await pimWrite(ctx, { type: "SetCalendarPreferences", preferences }, "CalendarPreferences")
  ).preferences;
}

// ---------------------------------------------------------------------------
// Working time, week start, week numbers

/** The weekday of a date (`mon` … `sun`). */
export function weekdayOf(day: CalendarDate): Weekday {
  // `toDate` at noon UTC: the weekday of the calendar date in any zone.
  const index = (day.toDate("UTC").getUTCDay() + 6) % 7;
  return WEEKDAYS[index] ?? "mon";
}

/** Whether `day` is a working day. */
export function isWorkDay(prefs: CalendarPreferences, day: CalendarDate): boolean {
  return prefs.work_days.includes(weekdayOf(day));
}

/** Whether the minute of the day `minute` on `day` is working time (a work day, inside the hours). */
export function isWorkingTime(
  prefs: CalendarPreferences,
  day: CalendarDate,
  minute: number,
): boolean {
  const { start_minute, end_minute } = prefs.work_hours;
  return isWorkDay(prefs, day) && minute >= start_minute && minute < end_minute;
}

/**
 * The stretches of `day` outside working time, as `[start, end)` minutes
 * (shading of the time grid): the whole day for a day off, else before and
 * after the working hours.
 */
export function offHours(
  prefs: CalendarPreferences,
  day: CalendarDate,
): readonly (readonly [number, number])[] {
  if (!isWorkDay(prefs, day)) return [[0, MINUTES_PER_DAY]];
  const { start_minute, end_minute } = prefs.work_hours;
  const out: [number, number][] = [];
  if (start_minute > 0) out.push([0, start_minute]);
  if (end_minute < MINUTES_PER_DAY) out.push([end_minute, MINUTES_PER_DAY]);
  return out;
}

/**
 * The ISO 8601 week number of `day` (weeks start on Monday; week 1 holds
 * the year's first Thursday). Week numbers follow ISO whatever the
 * displayed week start is, as calendars in Europe print them.
 */
export function isoWeekNumber(day: CalendarDate): number {
  const date = day.toDate("UTC");
  const weekday = (date.getUTCDay() + 6) % 7;
  // The Thursday of this week decides the year.
  const thursday = new Date(date.getTime() + (3 - weekday) * DAY_MS);
  const firstThursday = new Date(Date.UTC(thursday.getUTCFullYear(), 0, 4));
  const firstWeekday = (firstThursday.getUTCDay() + 6) % 7;
  const weekOne = firstThursday.getTime() - firstWeekday * DAY_MS;
  return 1 + Math.round((thursday.getTime() - 3 * DAY_MS - weekOne) / (7 * DAY_MS));
}

/** The first weekday of the week: the preference, else `null` (the UI locale's). */
export function weekStartOf(prefs: CalendarPreferences): Weekday | null {
  return prefs.week_start;
}

// ---------------------------------------------------------------------------
// View zones

/** The zones a calendar view shows. */
export interface ViewZones {
  /** Events are placed in this zone. */
  zone: string;
  /** A second zone for the time grid's gutter, if any. */
  second: string | null;
}

/**
 * The view zones: a widget's layout option (`time-zone`, `secondary-zone`)
 * wins, then the preferences, then the system's zone. Unknown zones are
 * ignored.
 */
export function viewZonesOf(
  prefs: CalendarPreferences,
  layout: { zone?: string | null; second?: string | null } = {},
): ViewZones {
  const known = (z: string | null | undefined) => (z && isKnownZone(z) ? z : null);
  const zone = known(layout.zone) ?? known(prefs.time_zone) ?? localZone();
  const second = known(layout.second) ?? known(prefs.second_time_zone);
  return { zone, second: second === zone ? null : second };
}

// ---------------------------------------------------------------------------
// Tasks due in a range

/** Tasks due in `range` (open ones), by due; `[]` while loading. */
export function useTasksDue(
  range: { from: string; to: string } | null,
  lists: readonly CalendarId[] | null,
  tz: string,
): Task[] {
  const client = useClient();
  const query = useQuery({
    queryKey: prefsKeys.tasksDue(range?.from ?? "", range?.to ?? "", lists, tz),
    queryFn: async () =>
      (
        await client.request(
          {
            type: "ListTasksDue",
            from: range?.from ?? "",
            to: range?.to ?? "",
            tz,
            calendar_ids: lists ? [...lists] : null,
            include_completed: false,
          },
          "Tasks",
        )
      ).tasks,
    enabled: range !== null,
  });
  return query.data ?? [];
}

/**
 * The drag data type of a task dragged onto the time grid to block time
 * (S12): the sidebar's "Due this week" panel and the task list set it, the
 * time grid accepts it.
 */
export const TASK_DRAG_TYPE = "application/x-nail-task";

/** The payload of [`TASK_DRAG_TYPE`]: the task's id and title (the new event's title). */
export interface TaskDragPayload {
  taskId: string;
  title: string;
}

/** Puts a task on a drag. */
export function setTaskDrag(data: DataTransfer, payload: TaskDragPayload): void {
  data.setData(TASK_DRAG_TYPE, JSON.stringify(payload));
  data.effectAllowed = "copy";
}

/** The task of a drag, `null` when it carries none (or something malformed). */
export function taskOfDrag(data: DataTransfer): TaskDragPayload | null {
  const text = data.getData(TASK_DRAG_TYPE);
  if (!text) return null;
  try {
    const value: unknown = JSON.parse(text);
    if (typeof value !== "object" || value === null) return null;
    const { taskId, title } = value as Record<string, unknown>;
    return typeof taskId === "string" && typeof title === "string" ? { taskId, title } : null;
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Quick create from text

/**
 * Reads `text` as an event in `tz` (the core's `ParseEventText`); nothing
 * is saved. `locale` is the UI language (`null` = the core's default).
 */
export async function parseEventText(
  client: NailClient,
  text: string,
  tz: string,
  locale: string | null,
): Promise<ParsedEventText> {
  return (
    await client.request({ type: "ParseEventText", text, tz, now: null, locale }, "EventTextParsed")
  ).parsed;
}
