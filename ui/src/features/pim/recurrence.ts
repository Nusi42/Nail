/**
 * The recurrence editor's model: the common rules (daily, weekly on some
 * days, monthly on a day or on the n-th weekday, yearly; every n; never /
 * after n times / until a day) as a `SimpleRule`, converted to and from the
 * contract's structured `RecurrenceRule`. Rules the simple editor cannot
 * show (several rules, `BYSETPOS`, `BYHOUR`, …) stay untouched as
 * "custom" unless the user replaces them. Exceptions and extra dates of
 * the series are always kept.
 */
import { CalendarDate, CalendarDateTime, toZoned } from "@internationalized/date";
import type { PimTime, Recurrence, RecurrenceRule, Weekday, WeekdayNum } from "../../api/generated";
import type { MessageArgs } from "../../i18n/localizer";
import { WEEKDAYS, dateString, localString, parseDay, splitPimTime, utcString } from "./time";

export const SIMPLE_FREQUENCIES = ["daily", "weekly", "monthly", "yearly"] as const;
export type SimpleFrequency = (typeof SIMPLE_FREQUENCIES)[number];

/** How a monthly / yearly rule picks its day. */
export type MonthlyMode = "day" | "weekday";

export type SimpleEnd =
  { type: "never" } | { type: "count"; count: number } | { type: "until"; until: CalendarDate };

export interface SimpleRule {
  freq: SimpleFrequency;
  interval: number;
  /** Weekly: the days (empty = the start's weekday). */
  weekdays: Weekday[];
  /** Monthly / yearly: on the start's day of month, or its n-th weekday. */
  monthly: MonthlyMode;
  end: SimpleEnd;
}

/** What the editor shows for a series. */
export type RecurrenceChoice =
  | { type: "none" }
  | { type: "simple"; rule: SimpleRule }
  | { type: "custom"; recurrence: Recurrence };

/** The weekday of a date (`mon` … `sun`). */
export function weekdayOf(day: CalendarDate): Weekday {
  // 1970-01-05 was a Monday.
  const monday = new CalendarDate(1970, 1, 5);
  const index = (((day.compare(monday) % 7) + 7) % 7) as 0 | 1 | 2 | 3 | 4 | 5 | 6;
  return WEEKDAYS[index];
}

/** Which occurrence of its weekday in the month a day is (1–5, or -1 for the last). */
export function weekdayOrdinal(day: CalendarDate): { ordinal: number; last: boolean } {
  const ordinal = Math.floor((day.day - 1) / 7) + 1;
  const last = day.add({ days: 7 }).month !== day.month;
  return { ordinal, last };
}

/** A new rule for a series starting on `start`. */
export function defaultRule(freq: SimpleFrequency, start: CalendarDate): SimpleRule {
  return {
    freq,
    interval: 1,
    weekdays: freq === "weekly" ? [weekdayOf(start)] : [],
    monthly: "day",
    end: { type: "never" },
  };
}

function isEmpty(list: readonly unknown[]): boolean {
  return list.length === 0;
}

/** The editor's view of a series' recurrence. */
export function choiceOf(recurrence: Recurrence | null, start: PimTime): RecurrenceChoice {
  if (recurrence === null || recurrence.rules.length === 0) {
    return recurrence !== null && recurrence.dates.length > 0
      ? { type: "custom", recurrence }
      : { type: "none" };
  }
  const [rule] = recurrence.rules;
  const simple = recurrence.rules.length === 1 && rule ? simpleOf(rule, start) : null;
  return simple !== null && recurrence.dates.length === 0
    ? { type: "simple", rule: simple }
    : { type: "custom", recurrence };
}

/** The simple form of a rule, `null` when the simple editor cannot show it. */
export function simpleOf(rule: RecurrenceRule, start: PimTime): SimpleRule | null {
  if (!(SIMPLE_FREQUENCIES as readonly string[]).includes(rule.freq)) return null;
  const freq = rule.freq as SimpleFrequency;
  if (
    !isEmpty(rule.by_second) ||
    !isEmpty(rule.by_minute) ||
    !isEmpty(rule.by_hour) ||
    !isEmpty(rule.by_year_day) ||
    !isEmpty(rule.by_week_no) ||
    !isEmpty(rule.by_set_pos) ||
    (rule.week_start !== null && rule.week_start !== "mon")
  ) {
    return null;
  }
  const startDay = splitPimTime(start).day;
  let end: SimpleEnd = { type: "never" };
  if (rule.end.type === "count") end = { type: "count", count: rule.end.count };
  if (rule.end.type === "until") {
    const day = parseDay(
      rule.end.until.type === "date" ? rule.end.until.date : rule.end.until.local,
    );
    if (day === null) return null;
    end = { type: "until", until: day };
  }
  const base = { freq, interval: Math.max(1, rule.interval), end };
  switch (freq) {
    case "daily":
      return isEmpty(rule.by_day) && isEmpty(rule.by_month_day) && isEmpty(rule.by_month)
        ? { ...base, weekdays: [], monthly: "day" }
        : null;
    case "weekly":
      if (!isEmpty(rule.by_month_day) || !isEmpty(rule.by_month)) return null;
      if (rule.by_day.some((d) => d.ordinal !== null)) return null;
      return {
        ...base,
        weekdays:
          rule.by_day.length > 0
            ? sortWeekdays(rule.by_day.map((d) => d.day))
            : [weekdayOf(startDay)],
        monthly: "day",
      };
    case "monthly":
    case "yearly": {
      const monthOk =
        freq === "monthly"
          ? isEmpty(rule.by_month)
          : isEmpty(rule.by_month) ||
            (rule.by_month.length === 1 && rule.by_month[0] === startDay.month);
      if (!monthOk) return null;
      if (
        isEmpty(rule.by_day) &&
        (isEmpty(rule.by_month_day) || sameList(rule.by_month_day, [startDay.day]))
      ) {
        return { ...base, weekdays: [], monthly: "day" };
      }
      if (rule.by_day.length === 1 && isEmpty(rule.by_month_day)) {
        const [only] = rule.by_day;
        const { ordinal, last } = weekdayOrdinal(startDay);
        if (
          only?.day === weekdayOf(startDay) &&
          (only.ordinal === ordinal || (only.ordinal === -1 && last))
        ) {
          return { ...base, weekdays: [], monthly: "weekday" };
        }
      }
      return null;
    }
  }
}

function sameList(a: readonly number[], b: readonly number[]): boolean {
  return a.length === b.length && a.every((v, i) => v === b[i]);
}

/** Weekdays in Monday-first order without duplicates. */
export function sortWeekdays(days: readonly Weekday[]): Weekday[] {
  return WEEKDAYS.filter((d) => days.includes(d));
}

/** An `UNTIL` of the right kind for a series starting at `start` (end of that day). */
export function untilFor(day: CalendarDate, start: PimTime): PimTime {
  switch (start.type) {
    case "date":
      return { type: "date", date: dateString(day) };
    case "floating":
      return {
        type: "floating",
        local: localString(new CalendarDateTime(day.year, day.month, day.day, 23, 59, 59)),
      };
    case "zoned": {
      const endOfDay = toZoned(
        new CalendarDateTime(day.year, day.month, day.day, 23, 59, 59),
        start.tz,
      );
      return { type: "zoned", local: utcString(endOfDay).slice(0, 19), tz: "UTC" };
    }
  }
}

const NO_PARTS = {
  by_second: [],
  by_minute: [],
  by_hour: [],
  by_month_day: [],
  by_year_day: [],
  by_week_no: [],
  by_month: [],
  by_set_pos: [],
  week_start: null,
} satisfies Partial<RecurrenceRule>;

/** The contract's rule for a simple rule of a series starting at `start`. */
export function ruleOf(simple: SimpleRule, start: PimTime): RecurrenceRule {
  const day = splitPimTime(start).day;
  const end: RecurrenceRule["end"] =
    simple.end.type === "never"
      ? { type: "never" }
      : simple.end.type === "count"
        ? { type: "count", count: Math.max(1, Math.trunc(simple.end.count)) }
        : { type: "until", until: untilFor(simple.end.until, start) };
  let byDay: WeekdayNum[] = [];
  let byMonthDay: number[] = [];
  if (simple.freq === "weekly") {
    const days = simple.weekdays.length > 0 ? sortWeekdays(simple.weekdays) : [weekdayOf(day)];
    byDay = days.map((d) => ({ day: d, ordinal: null }));
  } else if (simple.freq === "monthly" || simple.freq === "yearly") {
    if (simple.monthly === "weekday") {
      const { ordinal, last } = weekdayOrdinal(day);
      byDay = [
        { day: weekdayOf(day), ordinal: ordinal === 5 || (last && ordinal > 4) ? -1 : ordinal },
      ];
    } else {
      byMonthDay = [day.day];
    }
  }
  return {
    ...NO_PARTS,
    freq: simple.freq,
    interval: Math.max(1, Math.trunc(simple.interval) || 1),
    end,
    by_day: byDay,
    by_month_day: byMonthDay,
    by_month: simple.freq === "yearly" ? [day.month] : [],
  };
}

/**
 * The series' recurrence after the editor: `none` drops it, a simple rule
 * replaces the rules (keeping exceptions and extra dates), custom keeps it.
 */
export function recurrenceOf(
  choice: RecurrenceChoice,
  start: PimTime,
  previous: Recurrence | null,
): Recurrence | null {
  switch (choice.type) {
    case "none":
      return null;
    case "custom":
      return choice.recurrence;
    case "simple":
      return {
        rules: [ruleOf(choice.rule, start)],
        dates: previous?.dates ?? [],
        exceptions: previous?.exceptions ?? [],
      };
  }
}

/** Fluent id + arguments describing a simple rule (`pim-recurrence-summary`). */
export function describe(
  rule: SimpleRule,
  start: PimTime,
  formatDay: (day: CalendarDate) => string,
  weekdayName: (day: Weekday) => string,
): { id: string; args: MessageArgs } {
  const day = splitPimTime(start).day;
  const { ordinal, last } = weekdayOrdinal(day);
  const args: MessageArgs = {
    freq: rule.freq,
    interval: rule.interval,
    days: sortWeekdays(rule.weekdays.length > 0 ? rule.weekdays : [weekdayOf(day)])
      .map(weekdayName)
      .join(", "),
    mode: rule.monthly,
    monthday: day.day,
    ordinal: last && ordinal > 4 ? -1 : ordinal,
    weekday: weekdayName(weekdayOf(day)),
    end: rule.end.type,
    count: rule.end.type === "count" ? rule.end.count : 0,
    until: rule.end.type === "until" ? formatDay(rule.end.until) : "",
  };
  return { id: "pim-recurrence-summary", args };
}
