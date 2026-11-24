/**
 * One month as an ARIA grid of days (`table role="grid"`): the mini-month
 * widget, the date picker's popup and the year view use it. The week starts
 * as the UI locale says; weekday and month names come from the UI locale.
 *
 * Keyboard (one roving tab stop): arrows move by a day / week, Home / End
 * to the start / end of the week, Page Up / Page Down by a month (with
 * Shift by a year), Enter / Space choose the day. Moving out of the shown
 * month shows the next one (`onFocusDay` + `onMonth`).
 *
 * R7 (S5, C2; ADR 0246), all optional: the range on screen as a band
 * (`data-band`: single, start, middle, end), dots in calendar colours for
 * days with events (`<part>.dots` / `<part>.dot`), a heat level per day
 * (`data-heat` 0 … 4, the year view) and ISO week numbers as row headers
 * (`<part>.week-number`, with an empty `<part>.corner` above them).
 */
import type { CalendarDate } from "@internationalized/date";
import { useEffect, useRef, type KeyboardEvent } from "react";
import { useDateFormat } from "../../i18n/format";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { Swatch, geomId } from "./common";
import { dayDate, formatDay } from "./fmt";
import { isoWeekNumber } from "./prefs";
import { dateString, monthGrid, dayGridMove, type ViewRange } from "./time";
import type { Weekday } from "../../api/generated";
import { bandOf } from "./viewsModel";

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
  onFocusDay: (day: CalendarDate) => void;
  /** Choose a day. */
  onSelect: (day: CalendarDate) => void;
  /** Take focus when the focused day changes (keyboard use). */
  focusOnMount?: boolean;
  /** Use compact (narrow) weekday headers. */
  narrow?: boolean;
  /** Show the month's name above the grid (`<part>.title`; the year view). */
  caption?: string;
  /** The range the calendar views show (a band over its days). */
  band?: ViewRange | null;
  /** Colours of the events per day (`YYYY-MM-DD`; `""` = the theme's). */
  dots?: ReadonlyMap<string, readonly string[]>;
  /** Heat level per day (`YYYY-MM-DD` → 1 … 4). */
  heat?: ReadonlyMap<string, number>;
  /** Show ISO week numbers in front of the weeks. */
  weekNumbers?: boolean;
  /** The first weekday of a row (`null` = the UI locale's). */
  weekStart?: Weekday | null;
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
  focusOnMount = false,
  narrow = false,
  caption,
  band = null,
  dots,
  heat,
  weekNumbers = false,
  weekStart = null,
}: MonthGridProps) {
  const t = useT();
  const locale = useLocale();
  const format = useDateFormat();
  const weeks = monthGrid(month, locale, weekStart);
  const firstWeek = weeks[0] ?? [];
  const tableRef = useRef<HTMLTableElement>(null);
  const keyboard = useRef(false);
  const focusedKey = dateString(focused);

  useEffect(() => {
    if (!keyboard.current && !focusOnMount) return;
    keyboard.current = false;
    tableRef.current?.querySelector<HTMLElement>(`[data-day="${focusedKey}"]`)?.focus();
  }, [focusedKey, focusOnMount]);

  const onKeyDown = (event: KeyboardEvent<HTMLTableElement>) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onSelect(focused);
      return;
    }
    const next = dayGridMove(event.key, focused, locale, {
      pages: true,
      shift: event.shiftKey,
      weekStart,
    });
    if (next === null) return;
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    event.preventDefault();
    keyboard.current = true;
    onFocusDay(next);
  };

  const name =
    label ?? format.dateTime(dayDate(month), { month: "long", year: "numeric", timeZone: "UTC" });

  return (
    <table ref={tableRef} role="grid" data-part={part} aria-label={name} onKeyDown={onKeyDown}>
      {caption !== undefined && (
        <caption data-part={`${part}.title`} aria-hidden="true">
          {caption}
        </caption>
      )}
      <thead data-part={`${part}.head`}>
        <tr data-part={`${part}.weekdays`}>
          {weekNumbers && (
            <th scope="col" data-part={`${part}.corner`} abbr={t("pim-views-week-number")}>
              <span data-part={`${part}.corner.label`}>{t("pim-views-week-short")}</span>
            </th>
          )}
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
            {weekNumbers && (
              <th
                scope="row"
                data-part={`${part}.week-number`}
                aria-label={t("pim-views-week-label", {
                  week: isoWeekNumber(week[3] ?? week[0] ?? month),
                })}
              >
                {isoWeekNumber(week[3] ?? week[0] ?? month)}
              </th>
            )}
            {week.map((day) => {
              const key = dateString(day);
              const states = [
                day.month !== month.month ? "outside" : null,
                selected !== null && day.compare(selected) === 0 ? "selected" : null,
                day.compare(today) === 0 ? "today" : null,
                busy?.has(key) ? "busy" : null,
              ].filter(Boolean);
              const isFocused = day.compare(focused) === 0;
              const dayDots = dots?.get(key) ?? [];
              const level = heat?.get(key) ?? 0;
              return (
                <td
                  key={key}
                  role="gridcell"
                  data-part={`${part}.day`}
                  data-day={key}
                  data-state={states.length > 0 ? states.join(" ") : undefined}
                  data-band={bandOf(day, band) ?? undefined}
                  data-heat={heat ? String(level) : undefined}
                  aria-selected={selected !== null && day.compare(selected) === 0}
                  aria-current={day.compare(today) === 0 ? "date" : undefined}
                  aria-label={
                    dayDots.length > 0
                      ? t("pim-views-day-with-events", {
                          day: formatDay(format, day, { dateStyle: "full" }),
                        })
                      : formatDay(format, day, { dateStyle: "full" })
                  }
                  tabIndex={isFocused ? 0 : -1}
                  onClick={() => {
                    onFocusDay(day);
                    onSelect(day);
                  }}
                >
                  {dots ? (
                    <>
                      <span data-part={`${part}.number`}>
                        {format.dateTime(dayDate(day), { day: "numeric", timeZone: "UTC" })}
                      </span>
                      <span data-part={`${part}.dots`} aria-hidden="true">
                        {dayDots.map((color) => (
                          <Swatch
                            key={color}
                            geom={geomId(part, "dot", key, color)}
                            color={color || null}
                            part={`${part}.dot`}
                          />
                        ))}
                      </span>
                    </>
                  ) : (
                    format.dateTime(dayDate(day), { day: "numeric", timeZone: "UTC" })
                  )}
                </td>
              );
            })}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
