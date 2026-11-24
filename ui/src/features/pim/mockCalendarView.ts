/**
 * The mock core's side of R7 (ADR 0246): calendar preferences, tasks due in
 * a range and a small quick-create reader, so the calendar works in mock
 * and demo mode. `mock.ts` routes the requests here.
 *
 * The reader is a test double of the core's `nail-event-text`, not the
 * real thing: it knows "today" / "tomorrow", "all day", one time ("12:30",
 * "3pm", "9:15am") or a range ("3-4pm"), and a trailing "at <place>".
 * Enough for the UI's preview and tests; the core answers everything else.
 */
import type {
  CalendarPreferences,
  FreeTimeHours,
  ParsedEventText,
  ParsedSpan,
  PimIssue,
  PimTime,
  Task,
} from "../../api/generated";
import { DEFAULT_CALENDAR_PREFERENCES, DEFAULT_EVENT_MINUTES } from "../../api/generated";
import {
  dateString,
  instantMs,
  isKnownZone,
  localString,
  pimTimeIn,
  todayIn,
  toZoned,
} from "./time";
import { weekdayOf, workHoursValid } from "./prefs";
import type { CalendarDate, ZonedDateTime } from "@internationalized/date";

/**
 * The stretches of `[fromMs, toMs)` that `FindFreeTime` with `hours`
 * searches (R8, ADR 0251; the core's `within_hours`): the whole range for
 * `any`, else the hours of each chosen day in `tz` (local wall times, so
 * summer-time days keep their hours; a skipped time moves past the gap).
 */
export function freeTimeWindows(
  hours: FreeTimeHours,
  prefs: CalendarPreferences,
  fromMs: number,
  toMs: number,
  tz: string,
): (readonly [number, number])[] {
  if (hours.kind === "any") return [[fromMs, toMs]];
  const { work_hours, work_days } = hours.kind === "working-hours" ? prefs : hours;
  const out: (readonly [number, number])[] = [];
  const at = (day: CalendarDate, minute: number): ZonedDateTime =>
    minute >= 24 * 60
      ? toZoned(day.add({ days: 1 }), tz)
      : toZoned(day, tz).set({ hour: Math.floor(minute / 60), minute: minute % 60 });
  let day = todayIn(tz, fromMs);
  for (let guard = 0; guard < 400; guard++) {
    if (toZoned(day, tz).toDate().getTime() >= toMs) break;
    if (work_days.includes(weekdayOf(day))) {
      const open = Math.max(fromMs, at(day, work_hours.start_minute).toDate().getTime());
      const close = Math.min(toMs, at(day, work_hours.end_minute).toDate().getTime());
      if (close > open) out.push([open, close]);
    }
    day = day.add({ days: 1 });
  }
  return out;
}

/** A fresh copy of the default preferences (the mock state owns it). */
export function defaultPreferences(): CalendarPreferences {
  return {
    ...DEFAULT_CALENDAR_PREFERENCES,
    work_hours: { ...DEFAULT_CALENDAR_PREFERENCES.work_hours },
    work_days: [...DEFAULT_CALENDAR_PREFERENCES.work_days],
  };
}

/** The first problem with preferences, like `CalendarPreferences::issues` and the core's zone check. */
export function preferencesIssue(prefs: CalendarPreferences): PimIssue | null {
  if (!workHoursValid(prefs.work_hours)) return "invalid-work-hours";
  if (new Set(prefs.work_days).size !== prefs.work_days.length) return "too-many";
  for (const zone of [prefs.time_zone, prefs.second_time_zone]) {
    if (zone !== null && !isKnownZone(zone)) return "invalid-time-zone";
  }
  return null;
}

/** Tasks of `tasks` due in `[from, to)` (dates placed in `tz`), by due. */
export function tasksDue(
  tasks: readonly Task[],
  from: string,
  to: string,
  tz: string,
  includeCompleted: boolean,
): Task[] {
  const fromMs = instantMs(from);
  const toMs = instantMs(to);
  return tasks
    .filter(
      (t) =>
        includeCompleted || (t.fields.status !== "completed" && t.fields.status !== "cancelled"),
    )
    .flatMap((task) => {
      if (task.fields.due === null) return [];
      const at = pimTimeIn(task.fields.due, tz).toDate().getTime();
      return fromMs <= at && at < toMs ? [{ at, task }] : [];
    })
    .sort((a, b) => a.at - b.at)
    .map((d) => d.task);
}

const DAY_WORDS: Record<string, number> = { today: 0, tomorrow: 1 };
const DAY_RE = /\b(today|tomorrow)\b/i;
const ALL_DAY_RE = /\ball day\b/i;
const TIME_RE = /\b(\d{1,2})(?::(\d{2}))?(?:\s*-\s*(\d{1,2})(?::(\d{2}))?)?\s*(am|pm)?\b/i;
const PLACE_RE = /\bat\s+(.+)$/i;

/** One match as a span of `kind`. */
function spanOf(match: RegExpExecArray, kind: ParsedSpan["kind"]): ParsedSpan {
  return { kind, start: match.index, end: match.index + match[0].length };
}

/** Minutes after midnight of `hour[:minute][am|pm]`, `null` when out of range. */
function minuteOf(
  hour: string,
  minute: string | undefined,
  meridiem: string | undefined,
): number | null {
  let h = Number(hour);
  const m = minute === undefined ? 0 : Number(minute);
  const half = meridiem?.toLowerCase();
  if (half === "pm" && h < 12) h += 12;
  if (half === "am" && h === 12) h = 0;
  return h < 24 && m < 60 ? h * 60 + m : null;
}

/**
 * Reads `text` like the core would for the words the mock knows (see the
 * module comment); `now` is a Unix time in milliseconds.
 */
export function mockParseEventText(text: string, tz: string, now: number): ParsedEventText {
  const spans: ParsedSpan[] = [];
  let location: string | null = null;
  const place = PLACE_RE.exec(text);
  if (place?.[1]) {
    location = place[1].trim();
    spans.push(spanOf(place, "location"));
  }
  const head = place ? text.slice(0, place.index) : text;
  let day = todayIn(tz, now);
  let dated = false;
  const dayMatch = DAY_RE.exec(head);
  if (dayMatch?.[1]) {
    day = day.add({ days: DAY_WORDS[dayMatch[1].toLowerCase()] ?? 0 });
    dated = true;
    spans.push(spanOf(dayMatch, "date"));
  }
  const allDay = ALL_DAY_RE.exec(head);
  if (allDay) spans.push(spanOf(allDay, "all-day"));
  const time = allDay ? null : TIME_RE.exec(head);
  const startMinute = time?.[1] ? minuteOf(time[1], time[2], time[5]) : null;
  const endMinute = time?.[3] ? minuteOf(time[3], time[4], time[5]) : null;
  if (time && startMinute !== null) spans.push(spanOf(time, "time"));
  spans.sort((a, b) => a.start - b.start);

  let start: PimTime | null = null;
  let end: PimTime | null = null;
  if (startMinute !== null) {
    const at = toZoned(day, tz).add({ minutes: startMinute });
    const length =
      endMinute !== null && endMinute > startMinute
        ? endMinute - startMinute
        : DEFAULT_EVENT_MINUTES;
    start = { type: "zoned", local: localString(at), tz };
    end = { type: "zoned", local: localString(at.add({ minutes: length })), tz };
  } else if (dated || allDay) {
    start = { type: "date", date: dateString(day) };
    end = { type: "date", date: dateString(day.add({ days: 1 })) };
  }
  let title = "";
  let at = 0;
  for (const span of spans) {
    title += text.slice(at, span.start);
    at = span.end;
  }
  title += text.slice(at);
  return {
    title: title.replace(/\s+/g, " ").trim(),
    start,
    end,
    location,
    recurrence: null,
    spans,
  };
}
