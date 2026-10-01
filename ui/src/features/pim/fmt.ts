/**
 * Date texts of the PIM feature, always through `i18n/format.ts` (the UI
 * locale) with an explicit zone: calendar days are formatted as UTC noon
 * so no zone can move them to another day; instants in the viewer's zone.
 */
import type { CalendarDate } from "@internationalized/date";
import type { PimTime, Weekday } from "../../api/generated";
import type { DateFormatter } from "../../i18n/format";
import { WEEKDAYS, instantMs, pimTimeIn, splitPimTime } from "./time";

/** A calendar day as a `Date` that formats as that day in UTC. */
export function dayDate(day: CalendarDate): Date {
  return new Date(Date.UTC(day.year, day.month - 1, day.day, 12));
}

/** Formats a calendar day. */
export function formatDay(
  format: DateFormatter,
  day: CalendarDate,
  options: Intl.DateTimeFormatOptions = { dateStyle: "medium" },
): string {
  return format.dateTime(dayDate(day), { ...options, timeZone: "UTC" });
}

/** Formats an instant (ms or RFC 3339) in a zone. */
export function formatInstant(
  format: DateFormatter,
  value: number | string,
  tz: string,
  options: Intl.DateTimeFormatOptions = { hour: "numeric", minute: "2-digit" },
): string {
  const ms = typeof value === "number" ? value : instantMs(value);
  return format.dateTime(ms, { ...options, timeZone: tz });
}

/** A time of day (`9:30 AM` / `09:30`) of an instant in a zone. */
export function formatClock(format: DateFormatter, value: number | string, tz: string): string {
  return formatInstant(format, value, tz, { hour: "numeric", minute: "2-digit" });
}

/** A whole `PimTime` for people: the day (all-day) or day and time in `tz`. */
export function formatPimTime(format: DateFormatter, time: PimTime, tz: string): string {
  if (time.type === "date") return formatDay(format, splitPimTime(time).day);
  return formatInstant(format, pimTimeIn(time, tz).toDate().getTime(), tz, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

/** The name of a weekday (`Mon` / `Monday`) in the UI locale. */
export function weekdayName(
  format: DateFormatter,
  day: Weekday,
  style: "short" | "long" | "narrow" = "long",
): string {
  // 2024-01-01 was a Monday.
  const index = WEEKDAYS.indexOf(day);
  return format.dateTime(new Date(Date.UTC(2024, 0, 1 + index, 12)), {
    weekday: style,
    timeZone: "UTC",
  });
}

/** An hour label (`9 AM` / `09`) for time grid gutters. */
export function formatHour(format: DateFormatter, hour: number): string {
  return format.dateTime(new Date(Date.UTC(2024, 0, 1, hour, 0)), {
    hour: "numeric",
    timeZone: "UTC",
  });
}

/** `9:30` style label for minutes after midnight. */
export function formatMinutes(format: DateFormatter, minutes: number): string {
  return format.dateTime(new Date(Date.UTC(2024, 0, 1, 0, minutes)), {
    hour: "numeric",
    minute: "2-digit",
    timeZone: "UTC",
  });
}

/** A short zone name (`CEST`, `GMT-4`) for an instant. */
export function zoneLabel(format: DateFormatter, ms: number, tz: string): string {
  const text = format.dateTime(ms, { timeZone: tz, timeZoneName: "short", hour: "numeric" });
  const parts = text.split(/\s+/);
  return parts[parts.length - 1] ?? tz;
}
