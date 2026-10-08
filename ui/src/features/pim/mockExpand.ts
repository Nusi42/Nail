/**
 * Recurrence expansion for the mock core (the real one is `nail-ical`):
 * enough of RFC 5545 for realistic UI tests — DAILY / WEEKLY / MONTHLY /
 * YEARLY with INTERVAL, BYDAY (with ordinals for monthly / yearly),
 * BYMONTHDAY, BYMONTH, COUNT / UNTIL, RDATE, EXDATE and overrides.
 * Occurrences are computed in the series' own wall-clock frame, so a
 * 09:00 Berlin meeting stays at 09:00 across the change of summer time.
 */
import {
  CalendarDate,
  CalendarDateTime,
  fromAbsolute,
  toCalendarDate,
} from "@internationalized/date";
import type {
  CalendarEvent,
  EventFields,
  EventInstance,
  PimTime,
  RecurrenceRule,
  Weekday,
} from "../../api/generated";
import { weekdayOf } from "./recurrence";
import { WEEKDAYS, likeAt, localString, parseDay, parseLocal, pimTimeIn, utcString } from "./time";
import { DAY_MS } from "../../state/clock";

/** Most occurrences one series produces in the mock. */
const MAX_OCCURRENCES = 2000;

/** The wall-clock moment of a `PimTime` in its own frame. */
function frameOf(time: PimTime): CalendarDateTime {
  if (time.type === "date") {
    const day = parseDay(time.date) ?? new CalendarDate(1970, 1, 1);
    return new CalendarDateTime(day.year, day.month, day.day);
  }
  return parseLocal(time.local) ?? new CalendarDateTime(1970, 1, 1);
}

/** Instant (ms) of a `PimTime`, dates and floating times seen in `tz`. */
export function instantOf(time: PimTime, tz: string): number {
  return pimTimeIn(time, tz).toDate().getTime();
}

/** Whether two recurrence ids name the same occurrence. */
export function sameOccurrence(a: PimTime, b: PimTime): boolean {
  if (a.type === "date" || b.type === "date") {
    return a.type === "date" && b.type === "date" && a.date === b.date;
  }
  return instantOf(a, "UTC") === instantOf(b, "UTC");
}

function daysInMonth(year: number, month: number): number {
  return new CalendarDate(year, month, 1).add({ months: 1 }).add({ days: -1 }).day;
}

/** Days of `month` matching `BYMONTHDAY` / `BYDAY` (ordinals allowed). */
function daysInMonthFor(
  rule: RecurrenceRule,
  year: number,
  month: number,
  start: CalendarDate,
): number[] {
  const count = daysInMonth(year, month);
  let days: number[] = [];
  if (rule.by_month_day.length > 0) {
    days = rule.by_month_day
      .map((d) => (d > 0 ? d : count + d + 1))
      .filter((d) => d >= 1 && d <= count);
  }
  if (rule.by_day.length > 0) {
    const matches: number[] = [];
    for (const { day, ordinal } of rule.by_day) {
      const all: number[] = [];
      for (let d = 1; d <= count; d++) {
        if (weekdayOf(new CalendarDate(year, month, d)) === day) all.push(d);
      }
      if (ordinal === null) matches.push(...all);
      else {
        const picked = ordinal > 0 ? all[ordinal - 1] : all[all.length + ordinal];
        if (picked !== undefined) matches.push(picked);
      }
    }
    days = days.length > 0 ? days.filter((d) => matches.includes(d)) : matches;
  }
  if (rule.by_month_day.length === 0 && rule.by_day.length === 0) {
    days = start.day <= count ? [start.day] : [];
  }
  return [...new Set(days)].sort((a, b) => a - b);
}

/** Candidate dates of period `k` of a rule (before COUNT / UNTIL). */
function periodDates(rule: RecurrenceRule, start: CalendarDate, k: number): CalendarDate[] {
  const step = Math.max(1, rule.interval);
  switch (rule.freq) {
    case "daily": {
      const day = start.add({ days: k * step });
      if (rule.by_month.length > 0 && !rule.by_month.includes(day.month)) return [];
      if (rule.by_day.length > 0 && !rule.by_day.some((d) => d.day === weekdayOf(day))) return [];
      return [day];
    }
    case "weekly": {
      const startIndex = WEEKDAYS.indexOf(weekdayOf(start));
      const monday = start.add({ days: -startIndex + k * step * 7 });
      const days: Weekday[] =
        rule.by_day.length > 0 ? rule.by_day.map((d) => d.day) : [weekdayOf(start)];
      return WEEKDAYS.flatMap((w, i) => (days.includes(w) ? [monday.add({ days: i })] : []));
    }
    case "monthly": {
      const first = new CalendarDate(start.year, start.month, 1).add({ months: k * step });
      if (rule.by_month.length > 0 && !rule.by_month.includes(first.month)) return [];
      return daysInMonthFor(rule, first.year, first.month, start).map((d) => first.set({ day: d }));
    }
    case "yearly": {
      const year = start.year + k * step;
      const months = rule.by_month.length > 0 ? rule.by_month : [start.month];
      return months.flatMap((month) =>
        daysInMonthFor(rule, year, month, start).map((d) => new CalendarDate(year, month, d)),
      );
    }
    default:
      // Sub-daily rules are not expanded by the mock: the first occurrence only.
      return k === 0 ? [start] : [];
  }
}

/** The original starts of a series' occurrences up to `untilMs` (in `tz`). */
export function occurrenceStarts(event: CalendarEvent, untilMs: number, tz: string): PimTime[] {
  const first = event.fields.start;
  const recurrence = event.recurrence;
  if (recurrence === null) return [first];
  const frame = frameOf(first);
  const startDay = toCalendarDate(frame);
  const startMs = instantOf(first, tz);
  const out: PimTime[] = [];
  const seen = new Set<string>();
  const push = (time: PimTime) => {
    const key = String(instantOf(time, tz));
    if (seen.has(key)) return;
    seen.add(key);
    out.push(time);
  };
  for (const rule of recurrence.rules) {
    let produced = 0;
    const ruleUntil =
      rule.end.type === "until" ? instantOf(rule.end.until, tz) : Number.POSITIVE_INFINITY;
    let done = false;
    for (let k = 0; !done && k < MAX_OCCURRENCES; k++) {
      const dates = periodDates(rule, startDay, k);
      for (const day of dates) {
        const time = likeAt(
          first,
          new CalendarDateTime(
            day.year,
            day.month,
            day.day,
            frame.hour,
            frame.minute,
            frame.second,
          ),
        );
        const ms = instantOf(time, tz);
        if (ms < startMs) continue;
        if (ms > ruleUntil || ms > untilMs) {
          done = true;
          break;
        }
        push(time);
        produced++;
        if (rule.end.type === "count" && produced >= rule.end.count) {
          done = true;
          break;
        }
      }
      // Periods that produce nothing for a long time end the search.
      if (dates.length === 0 && k > 400 && produced === 0) done = true;
    }
  }
  if (recurrence.rules.length === 0) push(first);
  for (const extra of recurrence.dates) push(extra);
  return out
    .filter((time) => !recurrence.exceptions.some((ex) => sameOccurrence(ex, time)))
    .sort((a, b) => instantOf(a, tz) - instantOf(b, tz));
}

/** Duration of the series' master in its own frame (minutes, or days for dates). */
function durationOf(fields: EventFields): { days: number; minutes: number } {
  if (fields.start.type === "date") {
    const a = parseDay(fields.start.date);
    const b = fields.end.type === "date" ? parseDay(fields.end.date) : null;
    return { days: a && b ? Math.max(1, b.compare(a)) : 1, minutes: 0 };
  }
  const a = frameOf(fields.start);
  const b = frameOf(fields.end);
  const minutes = Math.round(
    (Date.UTC(b.year, b.month - 1, b.day, b.hour, b.minute) -
      Date.UTC(a.year, a.month - 1, a.day, a.hour, a.minute)) /
      60_000,
  );
  return { days: 0, minutes: Math.max(0, minutes) };
}

/** The end of an occurrence starting at `start` with the master's duration. */
export function endFor(fields: EventFields, start: PimTime): PimTime {
  const { days, minutes } = durationOf(fields);
  const frame = frameOf(start).add({ days, minutes });
  // The end keeps the master end's kind and zone (an event may end in another zone).
  if (fields.end.type === "zoned" && start.type === "zoned" && fields.end.tz !== start.tz) {
    const length = instantOf(fields.end, "UTC") - instantOf(fields.start, "UTC");
    const end = fromAbsolute(instantOf(start, "UTC") + length, fields.end.tz);
    return { type: "zoned", local: localString(end), tz: fields.end.tz };
  }
  return likeAt(start, frame);
}

/** The instances of one series overlapping `[fromMs, toMs)` for a viewer in `tz`. */
export function expandEvent(
  event: CalendarEvent,
  fromMs: number,
  toMs: number,
  tz: string,
  calendarReadOnly: boolean,
): EventInstance[] {
  const recurring = event.recurrence !== null;
  const instances: EventInstance[] = [];
  const add = (fields: EventFields, recurrenceId: PimTime | null, overridden: boolean) => {
    if (fields.status === "cancelled") return;
    const start = instantOf(fields.start, tz);
    const end = Math.max(instantOf(fields.end, tz), start);
    if (!(start < toMs && (end > fromMs || (end === start && start >= fromMs)))) return;
    instances.push({
      event_id: event.id,
      calendar_id: event.calendar_id,
      recurrence_id: recurrenceId,
      start: fields.start,
      end: fields.end,
      start_utc: utcString(start),
      end_utc: utcString(end),
      all_day: fields.start.type === "date",
      title: fields.title,
      location: fields.location,
      status: fields.status,
      transparency: fields.transparency,
      color: fields.color,
      recurring,
      overridden,
      has_attendees: fields.attendees.length > 0,
      has_alarms: fields.alarms.length > 0,
      read_only: event.read_only || calendarReadOnly,
    });
  };
  if (!recurring) {
    add(event.fields, null, false);
    return instances;
  }
  // Overrides may move an occurrence into the range from far away: look a year around.
  const starts = occurrenceStarts(event, toMs + 366 * DAY_MS, tz);
  for (const start of starts) {
    const override = event.overrides.find((o) => sameOccurrence(o.recurrence_id, start));
    if (override) add(override.fields, start, true);
    else add({ ...event.fields, start, end: endFor(event.fields, start) }, start, false);
  }
  return instances;
}
