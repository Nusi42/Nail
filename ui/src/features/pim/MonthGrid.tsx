/**
 * One month as an ARIA grid of days (`table role="grid"`): the mini-month
 * widget, the date picker's popup and the year view use it. The week starts
 * as the UI locale says; weekday and month names come from the UI locale.
 *
 * Keyboard (one roving tab stop): arrows move by a day / week, Home / End
 * to the start / end of the week, Page Up / Page Down by a month (with
 * Shift by a year), Enter / Space choose the day. Moving out of the shown
 * month shows the next one (`onFocusDay` + `onMonth`).
 */
import type { CalendarDate } from "@internationalized/date";
import { useEffect, useRef, type KeyboardEvent } from "react";
import { useDateFormat } from "../../i18n/format";
import { useLocale } from "../../i18n/I18nProvider";
import { dayDate, formatDay } from "./fmt";
import { dateString, monthGrid, startOfWeekFor } from "./time";

export interface MonthGridProps {
  /** Any day of the month shown. */
  month: CalendarDate;
  /** The chosen day, if any. */
  selected: CalendarDate | null;
  /** The day with the tab stop. */
  focused: CalendarDate;
  /** Today (marked `aria-current="date"`). */
  today: CalendarDate;
  /** Days that have something (`data-state~="busy"`). */
  busy?: ReadonlySet<string>;
  /** Accessible name of the grid (else the month's name). */
  label?: string;
  /** Part name prefix (`mini-month`, `date-picker`, `year-view.month`). */
  part: string;
  /** Move the tab stop. */
  onFocusDay(day: CalendarDate): void;
  /** Choose a day. */
  onSelect(day: CalendarDate): void;
  /** Take focus when the focused day changes (keyboard use). */
  autoFocus?: boolean;
  /** Use compact (narrow) weekday headers. */
  narrow?: boolean;
}

export function MonthGrid({
  month,
  selected,
  focused,
  today,
  busy,
  label,
  part,
  onFocusDay,
  onSelect,
  autoFocus = false,
  narrow = false,
}: MonthGridProps) {
  const locale = useLocale();
  const format = useDateFormat();
  const weeks = monthGrid(month, locale);
  const firstWeek = weeks[0] ?? [];
  const tableRef = useRef<HTMLTableElement>(null);
  const keyboard = useRef(false);
  const focusedKey = dateString(focused);

  useEffect(() => {
    if (!keyboard.current && !autoFocus) return;
    keyboard.current = false;
    tableRef.current?.querySelector<HTMLElement>(`[data-day="${focusedKey}"]`)?.focus();
  }, [focusedKey, autoFocus]);

  const onKeyDown = (event: KeyboardEvent<HTMLTableElement>) => {
    let next: CalendarDate;
    switch (event.key) {
      case "ArrowLeft":
        next = focused.add({ days: -1 });
        break;
      case "ArrowRight":
        next = focused.add({ days: 1 });
        break;
      case "ArrowUp":
        next = focused.add({ weeks: -1 });
        break;
      case "ArrowDown":
        next = focused.add({ weeks: 1 });
        break;
      case "Home":
        next = startOfWeekFor(focused, locale);
        break;
      case "End":
        next = startOfWeekFor(focused, locale).add({ days: 6 });
        break;
      case "PageUp":
        next = focused.add(event.shiftKey ? { years: -1 } : { months: -1 });
        break;
      case "PageDown":
        next = focused.add(event.shiftKey ? { years: 1 } : { months: 1 });
        break;
      case "Enter":
      case " ":
        event.preventDefault();
        onSelect(focused);
        return;
      default:
        return;
    }
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    event.preventDefault();
    keyboard.current = true;
    onFocusDay(next);
  };

  const name =
    label ?? format.dateTime(dayDate(month), { month: "long", year: "numeric", timeZone: "UTC" });

  return (
    <table ref={tableRef} role="grid" data-part={part} aria-label={name} onKeyDown={onKeyDown}>
      <thead data-part={`${part}.head`}>
        <tr data-part={`${part}.weekdays`}>
          {firstWeek.map((day) => (
            <th
              key={dateString(day)}
              scope="col"
              data-part={`${part}.weekday`}
              abbr={format.dateTime(dayDate(day), { weekday: "long", timeZone: "UTC" })}
            >
              {format.dateTime(dayDate(day), {
                weekday: narrow ? "narrow" : "short",
                timeZone: "UTC",
              })}
            </th>
          ))}
        </tr>
      </thead>
      <tbody data-part={`${part}.body`}>
        {weeks.map((week) => (
          <tr key={dateString(week[0] ?? month)} data-part={`${part}.week`}>
            {week.map((day) => {
              const key = dateString(day);
              const states = [
                day.month !== month.month ? "outside" : null,
                selected !== null && day.compare(selected) === 0 ? "selected" : null,
                day.compare(today) === 0 ? "today" : null,
                busy?.has(key) ? "busy" : null,
              ].filter(Boolean);
              const isFocused = day.compare(focused) === 0;
              return (
                <td
                  key={key}
                  role="gridcell"
                  data-part={`${part}.day`}
                  data-day={key}
                  data-state={states.length > 0 ? states.join(" ") : undefined}
                  aria-selected={selected !== null && day.compare(selected) === 0}
                  aria-current={day.compare(today) === 0 ? "date" : undefined}
                  aria-label={formatDay(format, day, { dateStyle: "full" })}
                  tabIndex={isFocused ? 0 : -1}
                  onClick={() => {
                    onFocusDay(day);
                    onSelect(day);
                  }}
                >
                  {format.dateTime(dayDate(day), { day: "numeric", timeZone: "UTC" })}
                </td>
              );
            })}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
