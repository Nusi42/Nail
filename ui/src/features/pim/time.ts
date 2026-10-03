/**
 * Dates and times of the PIM feature: `PimTime` ↔ `@internationalized/date`
 * values, view ranges (day, multi-day, week, work week, month, year,
 * agenda, free time), stepping, and the placement of events in a day
 * column. Pure functions; nothing here formats text for people (that goes
 * through `i18n/format.ts`, always in the UI locale).
 *
 * The week starts as the UI locale says (`startOfWeek(date, locale)`), so
 * a German UI starts weeks on Monday even in an English browser.
 */
import {
  CalendarDate,
  CalendarDateTime,
  type DayOfWeek,
  type ZonedDateTime,
  endOfMonth,
  fromAbsolute,
  getLocalTimeZone,
  isWeekend,
  parseAbsolute,
  parseDate,
  parseDateTime,
  startOfMonth,
  startOfWeek,
  startOfYear,
  toCalendarDate,
  toCalendarDateTime,
  toTimeZone,
  toZoned,
} from "@internationalized/date";
import type { EventInstance, PimTime, Weekday } from "../../api/generated";

/** The calendar views (`calendar view="…"`). */
export const VIEW_KINDS = [
  "day",
  "multi-day",
  "week",
  "work-week",
  "month",
  "year",
  "agenda",
  "free-time",
] as const;
export type ViewKind = (typeof VIEW_KINDS)[number];

export function isViewKind(value: unknown): value is ViewKind {
  return typeof value === "string" && (VIEW_KINDS as readonly string[]).includes(value);
}

/** Weekdays in RFC 5545 order starting Monday, as the contract names them. */
export const WEEKDAYS: readonly Weekday[] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

/** `@internationalized/date` names of the weekdays (`firstDayOfWeek`). */
const DAY_OF_WEEK: Record<Weekday, DayOfWeek> = {
  mon: "mon",
  tue: "tue",
  wed: "wed",
  thu: "thu",
  fri: "fri",
  sat: "sat",
  sun: "sun",
};

/** The viewer's zone (the system's), never the empty string. */
export function localZone(): string {
  try {
    return getLocalTimeZone() || "UTC";
  } catch {
    return "UTC";
  }
}

/** Every IANA zone the runtime knows (for zone pickers), `UTC` first. */
export function timeZoneNames(): string[] {
  let names: string[];
  try {
    names = Intl.supportedValuesOf("timeZone");
  } catch {
    names = [];
  }
  return ["UTC", ...names.filter((n) => n !== "UTC")];
}

/** Whether the runtime knows `tz`. */
export function isKnownZone(tz: string): boolean {
  try {
    fromAbsolute(0, tz);
    return true;
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// Conversions

const pad = (n: number, width = 2) => String(Math.trunc(n)).padStart(width, "0");

/** `YYYY-MM-DD`. */
export function dateString(date: CalendarDate | CalendarDateTime | ZonedDateTime): string {
  return `${pad(date.year, 4)}-${pad(date.month)}-${pad(date.day)}`;
}

/** `YYYY-MM-DDTHH:MM:SS` (wall clock, no zone). */
export function localString(value: CalendarDateTime | ZonedDateTime): string {
  return `${dateString(value)}T${pad(value.hour)}:${pad(value.minute)}:${pad(value.second)}`;
}

/** RFC 3339 UTC as the contract writes instants: `YYYY-MM-DDTHH:MM:SSZ`. */
export function utcString(value: ZonedDateTime | number | Date): string {
  const ms =
    typeof value === "number"
      ? value
      : value instanceof Date
        ? value.getTime()
        : value.toDate().getTime();
  return `${localString(fromAbsolute(ms, "UTC"))}Z`;
}

/** Parses `YYYY-MM-DD`, `null` when malformed. */
export function parseDay(text: string): CalendarDate | null {
  try {
    return parseDate(text.slice(0, 10));
  } catch {
    return null;
  }
}

/** Parses `YYYY-MM-DDTHH:MM:SS`, `null` when malformed. */
export function parseLocal(text: string): CalendarDateTime | null {
  try {
    return parseDateTime(text);
  } catch {
    return null;
  }
}

/** An instant (RFC 3339) in a zone. */
export function instantIn(utc: string, tz: string): ZonedDateTime {
  try {
    return parseAbsolute(utc, tz);
  } catch {
    return fromAbsolute(Date.parse(utc) || 0, tz);
  }
}

/** Milliseconds since the epoch of an instant string. */
export function instantMs(utc: string): number {
  return instantIn(utc, "UTC").toDate().getTime();
}

/**
 * When a `PimTime` happens, seen from `viewTz`: dates start at midnight in
 * the viewer's zone, floating times are wall-clock times there, zoned
 * times are converted.
 */
export function pimTimeIn(time: PimTime, viewTz: string): ZonedDateTime {
  switch (time.type) {
    case "date":
      return toZoned(parseDay(time.date) ?? new CalendarDate(1970, 1, 1), viewTz);
    case "floating":
      return toZoned(parseLocal(time.local) ?? new CalendarDateTime(1970, 1, 1), viewTz);
    case "zoned": {
      const local = parseLocal(time.local) ?? new CalendarDateTime(1970, 1, 1);
      const tz = isKnownZone(time.tz) ? time.tz : "UTC";
      return toTimeZone(toZoned(local, tz), viewTz);
    }
  }
}

/** Whether two times name the same moment (dates: the same day). */
export function sameTime(a: PimTime, b: PimTime): boolean {
  if (a.type === "date" || b.type === "date") {
    return a.type === "date" && b.type === "date" && a.date === b.date;
  }
  return pimTimeIn(a, "UTC").compare(pimTimeIn(b, "UTC")) === 0;
}

/** The zone a time is written in, `null` for dates and floating times. */
export function zoneOf(time: PimTime): string | null {
  return time.type === "zoned" ? time.tz : null;
}

/**
 * A `PimTime` of the same kind (and zone) as `like` for the instant
 * `value` (dates keep only the day in `value`'s own zone).
 */
export function sameKindAs(like: PimTime, value: ZonedDateTime): PimTime {
  switch (like.type) {
    case "date":
      return { type: "date", date: dateString(value) };
    case "floating":
      return { type: "floating", local: localString(value) };
    case "zoned":
      return {
        type: "zoned",
        local: localString(toTimeZone(value, isKnownZone(like.tz) ? like.tz : "UTC")),
        tz: like.tz,
      };
  }
}

/** Builds a `PimTime` from a day, an optional wall-clock time and a zone (`null` = floating). */
export function makePimTime(
  day: CalendarDate,
  time: { hour: number; minute: number } | null,
  tz: string | null,
): PimTime {
  if (time === null) return { type: "date", date: dateString(day) };
  const local = localString(
    new CalendarDateTime(day.year, day.month, day.day, time.hour, time.minute),
  );
  return tz === null ? { type: "floating", local } : { type: "zoned", local, tz };
}

/** The day and wall-clock time a `PimTime` is written with (its own zone). */
export function splitPimTime(time: PimTime): {
  day: CalendarDate;
  time: { hour: number; minute: number } | null;
  tz: string | null;
} {
  if (time.type === "date") {
    return { day: parseDay(time.date) ?? new CalendarDate(1970, 1, 1), time: null, tz: null };
  }
  const local = parseLocal(time.local) ?? new CalendarDateTime(1970, 1, 1);
  return {
    day: toCalendarDate(local),
    time: { hour: local.hour, minute: local.minute },
    tz: time.type === "zoned" ? time.tz : null,
  };
}

/** Minutes from `a` to `b` (both seen as instants). */
export function minutesBetween(a: ZonedDateTime, b: ZonedDateTime): number {
  return Math.round((b.toDate().getTime() - a.toDate().getTime()) / 60_000);
}

/**
 * Moves a `PimTime` by whole days and minutes of wall-clock time in its own
 * kind: dates move by days only; floating and zoned times keep their zone.
 */
export function shiftPimTime(time: PimTime, days: number, minutes: number): PimTime {
  if (time.type === "date") {
    const day = parseDay(time.date);
    return day ? { type: "date", date: dateString(day.add({ days })) } : time;
  }
  const local = parseLocal(time.local);
  if (!local) return time;
  const moved = local.add({ days, minutes });
  return time.type === "floating"
    ? { type: "floating", local: localString(moved) }
    : { type: "zoned", local: localString(moved), tz: time.tz };
}

/** Today in a zone. */
export function todayIn(tz: string, now = Date.now()): CalendarDate {
  return toCalendarDate(fromAbsolute(now, tz));
}

// ---------------------------------------------------------------------------
// View ranges

export interface ViewOptions {
  /** Days of `multi-day` and `agenda` (and `free-time`). */
  days: number;
  /** First day of a work week. */
  workWeekStart: Weekday;
  /** Days of a work week. */
  workWeekDays: number;
}

export const DEFAULT_VIEW_OPTIONS: ViewOptions = {
  days: 3,
  workWeekStart: "mon",
  workWeekDays: 5,
};

/** The days a view shows for an anchor day. */
export interface ViewRange {
  start: CalendarDate;
  /** Number of days (month: whole weeks around the month). */
  count: number;
}

export function viewRange(
  view: ViewKind,
  anchor: CalendarDate,
  locale: string,
  options: ViewOptions = DEFAULT_VIEW_OPTIONS,
): ViewRange {
  switch (view) {
    case "day":
      return { start: anchor, count: 1 };
    case "multi-day":
    case "agenda":
      return { start: anchor, count: Math.max(1, options.days) };
    case "week":
      return { start: startOfWeek(anchor, locale), count: 7 };
    case "work-week":
      return {
        start: startOfWeek(anchor, locale, DAY_OF_WEEK[options.workWeekStart]),
        count: Math.min(7, Math.max(1, options.workWeekDays)),
      };
    case "free-time":
      return workingDaysFrom(anchor, locale, Math.max(1, options.days));
    case "month": {
      const first = startOfWeek(startOfMonth(anchor), locale);
      const last = endOfMonth(anchor);
      const days = last.compare(first) + 1;
      return { start: first, count: Math.ceil(days / 7) * 7 };
    }
    case "year": {
      const first = startOfYear(anchor);
      return { start: first, count: first.add({ years: 1 }).compare(first) };
    }
  }
}

/** The anchor one page earlier (`-1`) or later (`1`). */
export function stepAnchor(
  view: ViewKind,
  anchor: CalendarDate,
  direction: 1 | -1,
  options: ViewOptions = DEFAULT_VIEW_OPTIONS,
): CalendarDate {
  switch (view) {
    case "day":
      return anchor.add({ days: direction });
    case "multi-day":
    case "agenda":
      return anchor.add({ days: direction * Math.max(1, options.days) });
    case "week":
    case "work-week":
    case "free-time":
      return anchor.add({ weeks: direction });
    case "month":
      return anchor.add({ months: direction });
    case "year":
      return anchor.add({ years: direction });
  }
}

/**
 * `count` working days from `anchor` (the free-time view): the range starts
 * at the first day on or after the anchor that is no weekend day in the
 * locale and spans as many calendar days as it takes to cover `count`
 * working days; {@link workingDaysOf} leaves the weekend days out again.
 */
export function workingDaysFrom(anchor: CalendarDate, locale: string, count: number): ViewRange {
  let start = anchor;
  for (let i = 0; i < 7 && isWeekend(start, locale); i++) start = start.add({ days: 1 });
  let days = 0;
  let working = 0;
  while (working < count && days < 7 * count) {
    if (!isWeekend(start.add({ days }), locale)) working++;
    days++;
  }
  return { start, count: Math.max(1, days) };
}

/** The days of a range that are no weekend days in the locale. */
export function workingDaysOf(range: ViewRange, locale: string): CalendarDate[] {
  const days = daysOf(range).filter((day) => !isWeekend(day, locale));
  return days.length > 0 ? days : daysOf(range);
}

/** A free stretch as RFC 3339 UTC instants (`FreeSlot`). */
export interface FreeStretch {
  start: string;
  end: string;
}

/**
 * The free stretches of one day's working hours: every stretch the core
 * found (`FindFreeTime` knows neither days nor working hours, so one stretch
 * can span several days) is cut to `[startHour, endHour)` of `day` in `tz`;
 * pieces shorter than `minutes` are dropped.
 */
export function freeStretchesOfDay(
  slots: readonly FreeStretch[],
  day: CalendarDate,
  tz: string,
  startHour: number,
  endHour: number,
  minutes: number,
): FreeStretch[] {
  const midnight = toZoned(day, tz);
  const from = midnight.add({ hours: startHour }).toDate().getTime();
  const to = midnight
    .add({ hours: Math.max(startHour, endHour) })
    .toDate()
    .getTime();
  const out: FreeStretch[] = [];
  for (const slot of slots) {
    const start = Math.max(from, instantMs(slot.start));
    const end = Math.min(to, instantMs(slot.end));
    if (end - start >= Math.max(1, minutes) * 60_000) {
      out.push({ start: utcString(start), end: utcString(end) });
    }
  }
  return out;
}

/** The days of a range. */
export function daysOf(range: ViewRange): CalendarDate[] {
  return Array.from({ length: range.count }, (_, i) => range.start.add({ days: i }));
}

/** The instants a range covers in `tz`: `[from, to)` as RFC 3339 UTC. */
export function rangeInstants(range: ViewRange, tz: string): { from: string; to: string } {
  return {
    from: utcString(toZoned(range.start, tz)),
    to: utcString(toZoned(range.start.add({ days: range.count }), tz)),
  };
}

/** Weeks of a month grid (7 days each). */
export function weeksOf(range: ViewRange): CalendarDate[][] {
  const days = daysOf(range);
  const weeks: CalendarDate[][] = [];
  for (let i = 0; i < days.length; i += 7) weeks.push(days.slice(i, i + 7));
  return weeks;
}

/** The first day of the week of `day` as the locale says. */
export function startOfWeekFor(day: CalendarDate, locale: string): CalendarDate {
  return startOfWeek(day, locale);
}

/** The days of one month as a grid of weeks starting as the locale says. */
export function monthGrid(month: CalendarDate, locale: string): CalendarDate[][] {
  return weeksOf(viewRange("month", startOfMonth(month), locale));
}

// ---------------------------------------------------------------------------
// Instances in days

/** Whether an instance touches a day (in the viewer's zone). */
export function overlapsDay(instance: EventInstance, day: CalendarDate, tz: string): boolean {
  const dayStart = toZoned(day, tz).toDate().getTime();
  const dayEnd = toZoned(day.add({ days: 1 }), tz)
    .toDate()
    .getTime();
  const start = instantMs(instance.start_utc);
  const end = Math.max(instantMs(instance.end_utc), start + 1);
  return start < dayEnd && end > dayStart;
}

/** Shown in the all-day row: all-day events and timed ones of 24 hours or more. */
export function isAllDayLike(instance: EventInstance): boolean {
  return (
    instance.all_day || instantMs(instance.end_utc) - instantMs(instance.start_utc) >= 86_400_000
  );
}

/** An event placed in a day column (fractions of the day, columns for overlaps). */
export interface PlacedInstance {
  instance: EventInstance;
  /** Minutes from the start of the day (clipped). */
  startMinute: number;
  /** Minutes from the start of the day (clipped, after `startMinute`). */
  endMinute: number;
  /** Wall-clock minutes of the day (1440). */
  dayMinutes: number;
  /** Overlap column and the number of columns of its cluster. */
  column: number;
  columns: number;
  /** Starts before / ends after this day. */
  continuesBefore: boolean;
  continuesAfter: boolean;
}

/** Minimum height of an event block, in minutes. */
export const MIN_BLOCK_MINUTES = 15;

/**
 * Places the timed instances of one day: clipped to the day, overlapping
 * events side by side (greedy columns per cluster of overlaps).
 */
export function placeDay(
  instances: readonly EventInstance[],
  day: CalendarDate,
  tz: string,
): PlacedInstance[] {
  const dayStart = toZoned(day, tz).toDate().getTime();
  const dayEnd = toZoned(day.add({ days: 1 }), tz)
    .toDate()
    .getTime();
  // Positions are wall-clock minutes (the grid shows 24 wall hours, also
  // on the days summer time starts or ends).
  const dayMinutes = 24 * 60;
  const items = instances
    .filter((i) => !isAllDayLike(i) && overlapsDay(i, day, tz))
    .map((instance) => {
      const start = instantMs(instance.start_utc);
      const end = instantMs(instance.end_utc);
      const startMinute = start <= dayStart ? 0 : minuteOfDay(fromAbsolute(start, tz));
      const endWall = end >= dayEnd ? dayMinutes : minuteOfDay(fromAbsolute(end, tz));
      const endMinute = Math.min(dayMinutes, Math.max(startMinute + MIN_BLOCK_MINUTES, endWall));
      return {
        instance,
        startMinute,
        endMinute,
        dayMinutes,
        column: 0,
        columns: 1,
        continuesBefore: start < dayStart,
        continuesAfter: end > dayEnd,
      };
    })
    .sort((a, b) => a.startMinute - b.startMinute || b.endMinute - a.endMinute);

  let cluster: PlacedInstance[] = [];
  let clusterEnd = -1;
  const columnEnds: number[] = [];
  const flush = () => {
    const columns = Math.max(1, columnEnds.length);
    for (const item of cluster) item.columns = columns;
    cluster = [];
    columnEnds.length = 0;
  };
  for (const item of items) {
    if (item.startMinute >= clusterEnd) flush();
    let column = columnEnds.findIndex((end) => end <= item.startMinute);
    if (column < 0) {
      column = columnEnds.length;
      columnEnds.push(item.endMinute);
    } else {
      columnEnds[column] = item.endMinute;
    }
    item.column = column;
    cluster.push(item);
    clusterEnd = Math.max(clusterEnd, item.endMinute);
  }
  flush();
  return items;
}

/** An all-day-row entry: which day columns it spans and its row. */
export interface SpanningInstance {
  instance: EventInstance;
  /** First and last day index within the view (inclusive). */
  first: number;
  last: number;
  row: number;
  continuesBefore: boolean;
  continuesAfter: boolean;
}

/** Lays out all-day-like instances over `days` (rows packed greedily). */
export function placeSpanning(
  instances: readonly EventInstance[],
  days: readonly CalendarDate[],
  tz: string,
): SpanningInstance[] {
  const out: SpanningInstance[] = [];
  const rowEnds: number[] = [];
  const candidates = instances
    .filter(isAllDayLike)
    .map((instance) => {
      const indexes = days.flatMap((day, i) => (overlapsDay(instance, day, tz) ? [i] : []));
      return { instance, indexes };
    })
    .filter((c) => c.indexes.length > 0)
    .sort(
      (a, b) => (a.indexes[0] ?? 0) - (b.indexes[0] ?? 0) || b.indexes.length - a.indexes.length,
    );
  for (const { instance, indexes } of candidates) {
    const first = indexes[0] ?? 0;
    const last = indexes[indexes.length - 1] ?? first;
    let row = rowEnds.findIndex((end) => end < first);
    if (row < 0) {
      row = rowEnds.length;
      rowEnds.push(last);
    } else {
      rowEnds[row] = last;
    }
    const firstDay = days[0];
    const lastDay = days[days.length - 1];
    out.push({
      instance,
      first,
      last,
      row,
      continuesBefore:
        first === 0 &&
        firstDay !== undefined &&
        instantMs(instance.start_utc) < toZoned(firstDay, tz).toDate().getTime(),
      continuesAfter:
        last === days.length - 1 &&
        lastDay !== undefined &&
        instantMs(instance.end_utc) >
          toZoned(lastDay.add({ days: 1 }), tz)
            .toDate()
            .getTime(),
    });
  }
  return out;
}

/** Instances touching a day, all-day ones first, then by start. */
export function instancesOfDay(
  instances: readonly EventInstance[],
  day: CalendarDate,
  tz: string,
): EventInstance[] {
  return instances
    .filter((i) => overlapsDay(i, day, tz))
    .sort(
      (a, b) =>
        Number(isAllDayLike(b)) - Number(isAllDayLike(a)) ||
        instantMs(a.start_utc) - instantMs(b.start_utc) ||
        a.title.localeCompare(b.title),
    );
}

/** The days (`YYYY-MM-DD` in `tz`) the instances touch. */
export function busyDays(instances: readonly EventInstance[], tz: string): Set<string> {
  const busy = new Set<string>();
  for (const instance of instances) {
    const end = instantMs(instance.end_utc);
    let day = toCalendarDate(instantIn(instance.start_utc, tz));
    for (let guard = 0; guard < 400; guard++) {
      busy.add(dateString(day));
      day = day.add({ days: 1 });
      if (toZoned(day, tz).toDate().getTime() >= end) break;
    }
  }
  return busy;
}

/** Snaps minutes to a step. */
export function snap(minutes: number, step = 15): number {
  return Math.round(minutes / step) * step;
}

/** The instant at `minute` minutes into `day` in `tz`. */
export function atMinute(day: CalendarDate, minute: number, tz: string): ZonedDateTime {
  return toZoned(day, tz).add({ minutes: minute });
}

/** Wall-clock minutes since midnight of an instant in a zone. */
export function minuteOfDay(value: ZonedDateTime): number {
  return value.hour * 60 + value.minute;
}

export { CalendarDate, CalendarDateTime, toCalendarDate, toCalendarDateTime, toZoned };
