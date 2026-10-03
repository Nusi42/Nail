/**
 * The day-based calendar views: month (a grid of weeks with the events of
 * each day; drag an event to another day), year (twelve month grids),
 * agenda (the occurrences of the next days as a list) and the free-time
 * timeline (free stretches of the working day from `FindFreeTime`, next to
 * the busy events; choosing one creates an event there).
 */
import type { CalendarDate } from "@internationalized/date";
import { useEffect, useRef, type KeyboardEvent } from "react";
import type { Calendar, EventInstance } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { useNow } from "../../state/clock";
import { useGeometry } from "../../theme/ThemeProvider";
import { geomId, useChangeContext, useFollowingDay } from "./common";
import { useDayDrag } from "./dayDrag";
import { EventItem } from "./EventItem";
import { dayDate, formatClock, formatDay } from "./fmt";
import { MonthGrid } from "./MonthGrid";
import { openNewEvent } from "./newEvent";
import { useFreeTime } from "./queries";
import {
  busyDays,
  dateString,
  daysOf,
  freeStretchesOfDay,
  instantIn,
  instancesOfDay,
  instantMs,
  isAllDayLike,
  rangeInstants,
  startOfWeekFor,
  todayIn,
  toZoned,
  weeksOf,
  workingDaysOf,
  type ViewRange,
} from "./time";
import { useLocale } from "../../i18n/I18nProvider";
import { CalendarDate as CalendarDateValue } from "@internationalized/date";

export interface DayViewProps {
  nodeKey: string;
  range: ViewRange;
  anchor: CalendarDate;
  instances: readonly EventInstance[];
  calendars: readonly Calendar[] | undefined;
  tz: string;
  onDay(day: CalendarDate): void;
}

// ---------------------------------------------------------------------------
// Month

export function MonthView({
  nodeKey,
  range,
  anchor,
  instances,
  calendars,
  tz,
  onDay,
  maxPerDay,
}: DayViewProps & { maxPerDay: number }) {
  const t = useT();
  const format = useDateFormat();
  const locale = useLocale();
  const ctx = useChangeContext();
  const today = todayIn(tz, useNow());
  const weeks = weeksOf(range);
  const [focused, setFocused] = useFollowingDay(anchor);
  const keyboard = useRef(false);
  const gridRef = useRef<HTMLTableElement>(null);
  const drag = useDayDrag(tz);

  useEffect(() => {
    if (!keyboard.current) return;
    keyboard.current = false;
    gridRef.current?.querySelector<HTMLElement>(`td[data-day="${dateString(focused)}"]`)?.focus();
  }, [focused]);

  const onKeyDown = (event: KeyboardEvent<HTMLTableElement>) => {
    if (!(event.target instanceof HTMLElement) || event.target.getAttribute("role") !== "gridcell")
      return;
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
      case "Enter":
        event.preventDefault();
        onDay(focused);
        return;
      default:
        return;
    }
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    event.preventDefault();
    const first = range.start;
    const last = range.start.add({ days: range.count - 1 });
    if (next.compare(first) < 0 || next.compare(last) > 0) {
      onDay(next);
      return;
    }
    keyboard.current = true;
    setFocused(next);
  };

  const firstWeek = weeks[0] ?? [];
  // The frame takes the view's height so the weeks share it (`height: 100%`
  // on the table needs a parent with a definite height).
  return (
    <div data-part="calendar.month.frame">
      <table
        ref={gridRef}
        role="grid"
        data-part="calendar.month"
        aria-label={format.dateTime(dayDate(anchor), {
          month: "long",
          year: "numeric",
          timeZone: "UTC",
        })}
        onKeyDown={onKeyDown}
        onPointerMove={drag.move}
        onPointerUp={drag.end}
        onPointerCancel={drag.cancel}
      >
        <thead data-part="calendar.month.head">
          <tr data-part="calendar.month.weekdays">
            {firstWeek.map((day) => (
              <th
                key={dateString(day)}
                scope="col"
                data-part="calendar.month.weekday"
                abbr={format.dateTime(dayDate(day), { weekday: "long", timeZone: "UTC" })}
              >
                {format.dateTime(dayDate(day), { weekday: "short", timeZone: "UTC" })}
              </th>
            ))}
          </tr>
        </thead>
        <tbody data-part="calendar.month.body">
          {weeks.map((week) => (
            <tr key={dateString(week[0] ?? anchor)} data-part="calendar.month.week">
              {week.map((day) => {
                const key = dateString(day);
                const dayInstances = instancesOfDay(instances, day, tz);
                const shown = dayInstances.slice(0, maxPerDay);
                const more = dayInstances.length - shown.length;
                const states = [
                  day.month !== anchor.month ? "outside" : null,
                  day.compare(today) === 0 ? "today" : null,
                  drag.over === key ? "drop-target" : null,
                ].filter(Boolean);
                return (
                  <td
                    key={key}
                    role="gridcell"
                    data-part="calendar.month.day"
                    data-day={key}
                    data-state={states.join(" ") || undefined}
                    aria-current={day.compare(today) === 0 ? "date" : undefined}
                    aria-label={t("pim-month-day-label", {
                      day: formatDay(format, day, { dateStyle: "full" }),
                      count: dayInstances.length,
                    })}
                    tabIndex={day.compare(focused) === 0 ? 0 : -1}
                    onDoubleClick={(event) => {
                      if (event.target === event.currentTarget) openNewEvent(ctx, day, null, tz);
                    }}
                  >
                    <span data-part="calendar.month.date" aria-hidden="true">
                      {format.dateTime(dayDate(day), { day: "numeric", timeZone: "UTC" })}
                    </span>
                    <ul data-part="calendar.month.events">
                      {shown.map((instance) => (
                        <li
                          key={`${instance.event_id}-${instance.start_utc}`}
                          data-part="calendar.month.item"
                        >
                          <EventItem
                            instance={instance}
                            tz={tz}
                            calendars={calendars}
                            geom={geomId(nodeKey, "m", key, instance.event_id, instance.start_utc)}
                            part="calendar.chip"
                            keys="days"
                            timeStyle="start"
                            states={[
                              isAllDayLike(instance) ? "spanning" : null,
                              drag.dragging?.event_id === instance.event_id &&
                              drag.dragging.start_utc === instance.start_utc
                                ? "dragging"
                                : null,
                            ].filter((x): x is string => x !== null)}
                            onDragStart={(event) => drag.start(event, instance, key)}
                            suppressClick={drag.suppressClick}
                          />
                        </li>
                      ))}
                    </ul>
                    {more > 0 && (
                      <button
                        type="button"
                        data-part="calendar.month.more"
                        onClick={() => onDay(day)}
                        aria-label={t("pim-more-events-label", {
                          count: more,
                          day: formatDay(format, day, { dateStyle: "full" }),
                        })}
                      >
                        {t("pim-more-events", { count: more })}
                      </button>
                    )}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Year

export function YearView({
  anchor,
  instances,
  tz,
  onDay,
}: Pick<DayViewProps, "anchor" | "instances" | "tz" | "onDay">) {
  const format = useDateFormat();
  const today = todayIn(tz, useNow());
  const [focused, setFocused] = useFollowingDay(anchor);
  const busy = busyDays(instances, tz);
  const months = Array.from({ length: 12 }, (_, i) => new CalendarDateValue(anchor.year, i + 1, 1));
  return (
    <div data-part="calendar.year">
      {months.map((month) => (
        <MonthGrid
          key={month.month}
          part="calendar.year.month"
          month={month}
          selected={null}
          focused={focused.month === month.month && focused.year === month.year ? focused : month}
          today={today}
          busy={busy}
          narrow
          caption={format.dateTime(dayDate(month), { month: "long", timeZone: "UTC" })}
          onFocusDay={setFocused}
          onSelect={onDay}
        />
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Agenda

export function AgendaList({
  nodeKey,
  range,
  instances,
  calendars,
  tz,
  part = "calendar.agenda",
  label,
}: Omit<DayViewProps, "anchor" | "onDay"> & { part?: string; label: string }) {
  const t = useT();
  const format = useDateFormat();
  const today = todayIn(tz, useNow());
  const days = daysOf(range)
    .map((day) => ({ day, items: instancesOfDay(instances, day, tz) }))
    .filter((d) => d.items.length > 0);
  if (days.length === 0) {
    return (
      <div data-part={part} data-state="empty" aria-label={label} role="region">
        <p data-part={`${part}.empty`}>{t("pim-agenda-empty")}</p>
      </div>
    );
  }
  return (
    <div data-part={part} aria-label={label} role="region">
      {days.map(({ day, items }) => (
        <section
          key={dateString(day)}
          data-part={`${part}.day`}
          data-day={dateString(day)}
          data-state={day.compare(today) === 0 ? "today" : undefined}
          aria-label={formatDay(format, day, { dateStyle: "full" })}
        >
          <h3 data-part={`${part}.date`}>
            {day.compare(today) === 0
              ? t("pim-today-with-date", {
                  date: formatDay(format, day, { weekday: "long", day: "numeric", month: "long" }),
                })
              : formatDay(format, day, { weekday: "long", day: "numeric", month: "long" })}
          </h3>
          <ul data-part={`${part}.list`}>
            {items.map((instance) => (
              <li key={`${instance.event_id}-${instance.start_utc}`} data-part={`${part}.item`}>
                <EventItem
                  instance={instance}
                  tz={tz}
                  calendars={calendars}
                  geom={geomId(
                    nodeKey,
                    "a",
                    dateString(day),
                    instance.event_id,
                    instance.start_utc,
                  )}
                  part={`${part}.event`}
                  keys="days"
                />
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Free time

function Segment({
  geom,
  part,
  left,
  width,
  children,
  onClick,
  label,
}: {
  geom: string;
  part: string;
  left: number;
  width: number;
  children?: React.ReactNode;
  onClick?(): void;
  label?: string;
}) {
  useGeometry(geom, {
    left: `${Math.round(left * 10000) / 100}%`,
    width: `${Math.max(0.5, Math.round(width * 10000) / 100)}%`,
  });
  if (onClick) {
    return (
      <button type="button" data-part={part} data-geom={geom} onClick={onClick} aria-label={label}>
        {children}
      </button>
    );
  }
  return (
    <span data-part={part} data-geom={geom} aria-hidden="true">
      {children}
    </span>
  );
}

export function FreeTimeView({
  nodeKey,
  range,
  instances,
  tz,
  minutes,
  dayStartHour,
  dayEndHour,
}: Omit<DayViewProps, "anchor" | "onDay" | "calendars"> & {
  minutes: number;
  dayStartHour: number;
  dayEndHour: number;
}) {
  const t = useT();
  const format = useDateFormat();
  const locale = useLocale();
  const ctx = useChangeContext();
  const instants = rangeInstants(range, tz);
  const free = useFreeTime(instants, minutes, tz);
  const spanMinutes = Math.max(60, (dayEndHour - dayStartHour) * 60);

  return (
    <div data-part="calendar.free-time" aria-busy={free.isPending}>
      <p data-part="calendar.free-time.intro">{t("pim-free-time-intro", { minutes })}</p>
      {workingDaysOf(range, locale).map((day) => {
        const dayStart = toZoned(day, tz).add({ hours: dayStartHour }).toDate().getTime();
        const position = (ms: number) => (ms - dayStart) / (spanMinutes * 60_000);
        const slots = freeStretchesOfDay(
          free.data ?? [],
          day,
          tz,
          dayStartHour,
          dayEndHour,
          minutes,
        );
        const busy = instances.filter(
          (i) =>
            !i.all_day &&
            i.transparency === "busy" &&
            dateString(instantIn(i.start_utc, tz)) === dateString(day),
        );
        const dayLabel = formatDay(format, day, { weekday: "long", day: "numeric", month: "long" });
        return (
          <section key={dateString(day)} data-part="calendar.free-time.day" aria-label={dayLabel}>
            <h3 data-part="calendar.free-time.date">{dayLabel}</h3>
            <div data-part="calendar.free-time.track">
              {busy.map((i) => {
                const left = Math.max(0, position(instantMs(i.start_utc)));
                const right = Math.min(1, position(instantMs(i.end_utc)));
                if (right <= 0 || left >= 1) return null;
                return (
                  <Segment
                    key={`${i.event_id}-${i.start_utc}`}
                    geom={geomId(nodeKey, "busy", i.event_id, i.start_utc)}
                    part="calendar.free-time.busy"
                    left={left}
                    width={right - left}
                  />
                );
              })}
              {slots.map((slot) => {
                const left = Math.max(0, position(instantMs(slot.start)));
                const right = Math.min(1, position(instantMs(slot.end)));
                const text = t("pim-time-range", {
                  start: formatClock(format, slot.start, tz),
                  end: formatClock(format, slot.end, tz),
                });
                return (
                  <Segment
                    key={slot.start}
                    geom={geomId(nodeKey, "free", slot.start)}
                    part="calendar.free-time.slot"
                    left={left}
                    width={Math.max(0, right - left)}
                    label={t("pim-free-slot-label", { day: dayLabel, time: text })}
                    onClick={() => {
                      const start = instantIn(slot.start, tz);
                      openNewEvent(ctx, day, { hour: start.hour, minute: start.minute }, tz);
                    }}
                  >
                    {text}
                  </Segment>
                );
              })}
            </div>
            {slots.length === 0 && !free.isPending && (
              <p data-part="calendar.free-time.none">{t("pim-free-time-none")}</p>
            )}
          </section>
        );
      })}
    </div>
  );
}
