/**
 * Date texts of the scheduling UI, through the PIM feature's formatting
 * (the UI locale, explicit zones; `i18n/format.ts` underneath).
 */
import type { MessageArgs } from "../../i18n/localizer";
import type { PimTime, Recurrence } from "../../api/generated";
import type { DateFormatter } from "../../i18n/format";
import { formatClock, formatDay, formatPimTime, weekdayName } from "../pim/fmt";
import { describe, simpleOf } from "../pim/recurrence";
import { dateString, instantIn, pimTimeIn, splitPimTime } from "../pim/time";

type T = (id: string, args?: MessageArgs) => string;

/** "Tuesday, 2 October, 14:30 – 15:30" (or a range over days / all-day days). */
export function timeRangeText(
  t: T,
  format: DateFormatter,
  start: PimTime,
  end: PimTime,
  tz: string,
): string {
  if (start.type === "date") {
    const first = splitPimTime(start).day;
    const last = end.type === "date" ? splitPimTime(end).day.add({ days: -1 }) : first;
    const day = formatDay(format, first, { dateStyle: "full" });
    return last.compare(first) <= 0
      ? t("scheduling-time-all-day", { day })
      : t("scheduling-time-days", {
          start: day,
          end: formatDay(format, last, { dateStyle: "full" }),
        });
  }
  const from = pimTimeIn(start, tz);
  const to = pimTimeIn(end, tz);
  if (dateString(from) === dateString(to)) {
    return t("scheduling-time-same-day", {
      day: formatDay(format, splitPimTime({ type: "date", date: dateString(from) }).day, {
        dateStyle: "full",
      }),
      start: formatClock(format, from.toDate().getTime(), tz),
      end: formatClock(format, to.toDate().getTime(), tz),
    });
  }
  return t("scheduling-time-range", {
    start: formatPimTime(format, start, tz),
    end: formatPimTime(format, end, tz),
  });
}

/** "14:30 – 15:30" of two instants (RFC 3339) in a zone. */
export function clockRange(t: T, format: DateFormatter, start: string, end: string, tz: string) {
  return t("scheduling-clock-range", {
    start: formatClock(format, start, tz),
    end: formatClock(format, end, tz),
  });
}

/** "Mon 2 Oct, 14:30 – 15:30" of two instants (RFC 3339) in a zone. */
export function instantRangeText(
  t: T,
  format: DateFormatter,
  start: string,
  end: string,
  tz: string,
): string {
  const day = instantIn(start, tz);
  return t("scheduling-time-same-day", {
    day: formatDay(format, splitPimTime({ type: "date", date: dateString(day) }).day, {
      weekday: "short",
      day: "numeric",
      month: "short",
    }),
    start: formatClock(format, start, tz),
    end: formatClock(format, end, tz),
  });
}

/** How a recurring invitation repeats ("Every week on Thursday, 10 times"). */
export function recurrenceText(
  t: T,
  format: DateFormatter,
  recurrence: Recurrence,
  start: PimTime,
): string {
  const rule = recurrence.rules[0];
  const simple = rule ? simpleOf(rule, start) : null;
  if (!simple) return t("scheduling-repeats-custom");
  const { id, args } = describe(
    simple,
    start,
    (day) => formatDay(format, day),
    (day) => weekdayName(format, day),
  );
  return t(id, args);
}
