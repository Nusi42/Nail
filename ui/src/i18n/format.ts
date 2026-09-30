/**
 * Dates, times and relative times in the UI locale (design.md, "Languages":
 * formatting follows the locale via `Intl`).
 *
 * The locale is always the one the localizer selected for the Fluent
 * catalogs (`useLocale()`: the settings / `?lang=` override, else the
 * browser's languages negotiated against the shipped catalogs), never the
 * browser's own formatting locale. Every date shown in the UI goes through
 * this module; `format.test.ts` fails on direct `Intl` date formatting
 * elsewhere in `ui/src`.
 */
import { useMemo } from "react";
import { useLocale } from "./I18nProvider";

/** `Intl.DateTimeFormat` options for the usual presentations. */
export const DATE_TIME = { dateStyle: "medium", timeStyle: "short" } as const;
export const DATE_TIME_SECONDS = { dateStyle: "medium", timeStyle: "medium" } as const;
export const SHORT_DATE_TIME_SECONDS = { dateStyle: "short", timeStyle: "medium" } as const;
export const FULL_DATE_TIME = { dateStyle: "full", timeStyle: "short" } as const;
export const DATE_ONLY = { dateStyle: "medium" } as const;

const cache = new Map<string, Intl.DateTimeFormat>();

/** A cached `Intl.DateTimeFormat` for `locale` and `options`. */
function dateTimeFormat(locale: string, options: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  const key = `${locale}\u0000${JSON.stringify(options)}`;
  let format = cache.get(key);
  if (!format) {
    format = new Intl.DateTimeFormat(locale, options);
    cache.set(key, format);
  }
  return format;
}

function toDate(value: Date | string | number): Date {
  return value instanceof Date ? value : new Date(value);
}

/**
 * A date and/or time in `locale`. A value that is not a valid date is
 * shown as it came (a string as is, anything else as empty text).
 */
export function formatDateTime(
  value: Date | string | number,
  locale: string,
  options: Intl.DateTimeFormatOptions = DATE_TIME,
): string {
  const date = toDate(value);
  if (Number.isNaN(date.getTime())) return typeof value === "string" ? value : "";
  return dateTimeFormat(locale, options).format(date);
}

const RELATIVE_STEPS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["second", 60],
  ["minute", 60],
  ["hour", 24],
  ["day", 7],
  ["week", 4.35],
  ["month", 12],
  ["year", Number.POSITIVE_INFINITY],
];

/** "5 minutes ago", "yesterday", "in 2 days", … in `locale`. */
export function formatRelative(date: Date, now: number, locale: string): string {
  const format = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  let amount = (date.getTime() - now) / 1000;
  if (Math.abs(amount) < 45) {
    return format.format(0, "second");
  }
  for (const [unit, size] of RELATIVE_STEPS) {
    if (Math.abs(amount) < size) {
      return format.format(Math.round(amount), unit);
    }
    amount /= size;
  }
  return format.format(Math.round(amount), "year");
}

/** Short date: time for today, day + month this year, else a numeric date. */
export function formatShortDate(date: Date, now: number, locale: string): string {
  const today = new Date(now);
  if (date.toDateString() === today.toDateString()) {
    return formatDateTime(date, locale, { hour: "2-digit", minute: "2-digit" });
  }
  if (date.getFullYear() === today.getFullYear()) {
    return formatDateTime(date, locale, { day: "numeric", month: "short" });
  }
  return formatDateTime(date, locale, { dateStyle: "short" });
}

/** Date formatting bound to one locale. */
export interface DateFormatter {
  /** The locale everything is formatted in. */
  readonly locale: string;
  /** See {@link formatDateTime}. */
  dateTime(value: Date | string | number, options?: Intl.DateTimeFormatOptions): string;
  /** See {@link formatRelative}. */
  relative(date: Date, now: number): string;
  /** See {@link formatShortDate}. */
  shortDate(date: Date, now: number): string;
}

/** A {@link DateFormatter} for `locale`. */
export function dateFormatter(locale: string): DateFormatter {
  return {
    locale,
    dateTime: (value, options) => formatDateTime(value, locale, options),
    relative: (date, now) => formatRelative(date, now, locale),
    shortDate: (date, now) => formatShortDate(date, now, locale),
  };
}

/** Date formatting in the UI locale (the Fluent locale in use). */
export function useDateFormat(): DateFormatter {
  const locale = useLocale();
  return useMemo(() => dateFormatter(locale), [locale]);
}
