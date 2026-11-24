/**
 * The day-based calendar views: month (a grid of weeks; drag an event to
 * another day), year (twelve month grids), agenda (the occurrences of the
 * next days as a list) and the free-time timeline (free stretches of the
 * working day from `FindFreeTime`, next to the busy events; choosing one
 * creates an event there).
 *
 * R7 (ADR 0246):
 * - Month (S8): events of a day or more are one bar across their days in
 *   each week row (lanes on top of the cells), timed ones a line of "dot,
 *   time, title" on their start day, "+N more" for the rest; today is the
 *   filled date pill only (no frame).
 * - Year (C2): each day shaded by the hours booked (`data-heat` 0 … 4),
 *   today ringed.
 * - Agenda (S7): a time column, the calendar's colour as a dot, title and
 *   place; sticky day headers with "Today" / "Tomorrow"; past events
 *   faded, a now marker between today's events, "until 1:00 AM" on the
 *   second day of an event crossing midnight; an empty range shows the
 *   empty state (C4).
 * - Free time (C1): an hour axis, busy blocks labelled with their titles,
 *   the reason when a whole day is busy and the free total per day.
 *
 * A click on an agenda entry opens the quick look (`setQuickLook`), a
 * double-click the editor; month cards are the shared `EventItem`.
 */
import type { CalendarDate } from "@internationalized/date";
import { useEffect, useRef, type KeyboardEvent, type ReactNode } from "react";
import type { Calendar, EventInstance } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { useNow } from "../../state/clock";
import { useGeometry } from "../../theme/ThemeProvider";
import { T } from "../../theme/tokens";
import { anchorOf } from "../../widgets/Popover";
import { requestDelete } from "./changes";
import { Swatch, colorOf, geomId, useChangeContext, useFollowingDay } from "./common";
import { useDayDrag } from "./dayDrag";
import { EventItem, useOpenInstance } from "./EventItem";
import { dayDate, formatClock, formatDay, formatHour } from "./fmt";
import { MonthGrid } from "./MonthGrid";
import { openNewEvent } from "./newEvent";
import { useCalendarPreferences, weekStartOf } from "./prefs";
import { useFreeTime } from "./queries";
import {
  dateString,
  freeStretchesOfDay,
  instantIn,
  instantMs,
  rangeInstants,
  dayGridMove,
  todayIn,
  toZoned,
  weeksOf,
  workingDaysOf,
  CalendarDate as CalendarDateValue,
  type ViewRange,
} from "./time";
import {
  agendaDays,
  bookedMinutes,
  busyAllDay,
  busyBlocks,
  freeMinutes,
  heatLevel,
  hourTicks,
  monthWeek,
  type AgendaRow,
  type MonthLane,
} from "./viewsModel";

export interface DayViewProps {
  nodeKey: string;
  range: ViewRange;
  anchor: CalendarDate;
  instances: readonly EventInstance[];
  calendars: readonly Calendar[] | undefined;
  tz: string;
  onDay: (day: CalendarDate) => void;
}

/** A position along a track as a geometry percentage. */
const percent = (fraction: number) => `${String(Math.round(fraction * 10000) / 100)}%`;

// ---------------------------------------------------------------------------
// Month

/**
 * The width of a bar across `days` cells of the month grid: the cell width
 * plus each further cell with its padding and the shared border.
 */
export function barWidth(days: number): string {
  return days <= 1
    ? "100%"
    : `calc(${String(days)} * 100% + ${String(days - 1)} * (2 * ${T.xxs} + 1px))`;
}

type DayDrag = ReturnType<typeof useDayDrag>;

/** Whether `instance` is the occurrence being dragged. */
const isDragged = (drag: DayDrag, instance: EventInstance) =>
  drag.dragging?.event_id === instance.event_id && drag.dragging.start_utc === instance.start_utc;

function MonthLaneItem({
  lane,
  nodeKey,
  dayKey,
  index,
  tz,
  calendars,
  drag,
}: {
  lane: MonthLane;
  nodeKey: string;
  dayKey: string;
  index: number;
  tz: string;
  calendars: readonly Calendar[] | undefined;
  drag: DayDrag;
}) {
  if (lane.kind === "gap") {
    return <li data-part="calendar.month.lane" data-kind="gap" aria-hidden="true" />;
  }
  const { instance, continuesBefore, continuesAfter } = lane.span;
  return (
    <li data-part="calendar.month.lane" data-kind="bar" data-lane={index}>
      <EventItem
        instance={instance}
        tz={tz}
        calendars={calendars}
        geom={geomId(nodeKey, "mb", dayKey, instance.event_id, instance.start_utc)}
        position={{ width: barWidth(lane.days) }}
        part="calendar.month.bar"
        keys="days"
        states={[
          continuesBefore ? "continues-before" : null,
          continuesAfter ? "continues-after" : null,
          isDragged(drag, instance) ? "dragging" : null,
        ].filter((x): x is string => x !== null)}
        onDragStart={(event) => {
          drag.start(event, instance, dayKey);
        }}
        suppressClick={drag.suppressClick}
      />
    </li>
  );
}

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
    if (event.key === "Enter") {
      event.preventDefault();
      onDay(focused);
      return;
    }
    const next = dayGridMove(event.key, focused, locale);
    if (next === null) return;
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
              {monthWeek(week, instances, tz, maxPerDay).map((cell) => {
                const { day } = cell;
                const key = dateString(day);
                const states = [
                  day.month !== anchor.month ? "outside" : null,
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
                      count: cell.total,
                    })}
                    tabIndex={day.compare(focused) === 0 ? 0 : -1}
                    onDoubleClick={(event) => {
                      if (event.target === event.currentTarget) openNewEvent(ctx, day, null, tz);
                    }}
                  >
                    <span data-part="calendar.month.date" aria-hidden="true">
                      {format.dateTime(dayDate(day), { day: "numeric", timeZone: "UTC" })}
                    </span>
                    {cell.lanes.length > 0 && (
                      <ul data-part="calendar.month.lanes">
                        {cell.lanes.map((lane, index) => (
                          <MonthLaneItem
                            key={
                              lane.kind === "bar"
                                ? lane.span.instance.start_utc
                                : `gap-${String(index)}`
                            }
                            lane={lane}
                            index={index}
                            nodeKey={nodeKey}
                            dayKey={key}
                            tz={tz}
                            calendars={calendars}
                            drag={drag}
                          />
                        ))}
                      </ul>
                    )}
                    <ul data-part="calendar.month.events">
                      {cell.lines.map((instance) => (
                        <li
                          key={`${instance.event_id}-${instance.start_utc}`}
                          data-part="calendar.month.item"
                        >
                          <EventItem
                            instance={instance}
                            tz={tz}
                            calendars={calendars}
                            geom={geomId(nodeKey, "m", key, instance.event_id, instance.start_utc)}
                            part="calendar.month.line"
                            keys="days"
                            timeStyle="start"
                            states={isDragged(drag, instance) ? ["dragging"] : []}
                            onDragStart={(event) => {
                              drag.start(event, instance, key);
                            }}
                            suppressClick={drag.suppressClick}
                          />
                        </li>
                      ))}
                    </ul>
                    {cell.more > 0 && (
                      <button
                        type="button"
                        data-part="calendar.month.more"
                        onClick={() => {
                          onDay(day);
                        }}
                        aria-label={t("pim-more-events-label", {
                          count: cell.more,
                          day: formatDay(format, day, { dateStyle: "full" }),
                        })}
                      >
                        {t("pim-more-events", { count: cell.more })}
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
  const prefs = useCalendarPreferences();
  const today = todayIn(tz, useNow());
  const [focused, setFocused] = useFollowingDay(anchor);
  const booked = bookedMinutes(instances, tz, prefs.work_hours);
  const heat = new Map([...booked].map(([day, minutes]) => [day, heatLevel(minutes)]));
  const busy = new Set([...heat].filter(([, level]) => level > 0).map(([day]) => day));
  const months = Array.from({ length: 12 }, (_, i) => new CalendarDateValue(anchor.year, i + 1, 1));
  return (
    <div data-part="calendar.year">
      {months.map((month) => (
        <MonthGrid
          key={month.month}
          part="calendar.year.grid"
          month={month}
          selected={null}
          focused={focused.month === month.month && focused.year === month.year ? focused : month}
          today={today}
          busy={busy}
          heat={heat}
          narrow
          weekNumbers={prefs.show_week_numbers}
          weekStart={weekStartOf(prefs)}
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

function AgendaEntry({
  row,
  part,
  geom,
  tz,
  calendars,
}: {
  row: AgendaRow;
  part: string;
  geom: string;
  tz: string;
  calendars: readonly Calendar[] | undefined;
}) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useChangeContext();
  const open = useOpenInstance();
  const { instance } = row;
  const title = instance.title || t("common-no-title");
  const start = formatClock(format, instance.start_utc, tz);
  const end = formatClock(format, instance.end_utc, tz);
  const time =
    row.time === "all-day"
      ? t("common-all-day")
      : row.time === "until"
        ? t("pim-views-agenda-until", { time: end })
        : start;
  const spoken = row.time === "timed" ? t("common-time-range", { start, end }) : time;
  const states = [
    row.past ? "past" : null,
    instance.my_status === "declined" ? "declined" : null,
    instance.my_status === "needs-action" ? "needs-action" : null,
  ].filter(Boolean);
  return (
    <button
      type="button"
      data-part={`${part}.entry`}
      data-event={instance.event_id}
      data-time={row.time}
      data-state={states.join(" ") || undefined}
      aria-label={t("pim-views-agenda-entry", {
        title,
        time: spoken,
        place: instance.location ?? "none",
        past: row.past ? "yes" : "no",
      })}
      onClick={(event) => {
        const pim = ctx.pim.getState();
        pim.select({ eventId: instance.event_id, recurrenceId: instance.recurrence_id });
        pim.setQuickLook({ instance, anchor: anchorOf(event.currentTarget) });
      }}
      onDoubleClick={() => {
        ctx.pim.getState().setQuickLook(null);
        open(instance);
      }}
      onKeyDown={(event) => {
        if ((event.key === "Delete" || event.key === "Backspace") && !instance.read_only) {
          event.preventDefault();
          void requestDelete(ctx, instance);
        }
      }}
    >
      <span data-part={`${part}.time`}>
        <span data-part={`${part}.time.start`}>{time}</span>
        {row.time === "timed" && <span data-part={`${part}.time.end`}>{end}</span>}
      </span>
      <Swatch
        geom={geom}
        color={colorOf(calendars, instance.calendar_id, instance.color)}
        part={`${part}.dot`}
      />
      <span data-part={`${part}.title`}>{title}</span>
      {instance.location && <span data-part={`${part}.place`}>{instance.location}</span>}
    </button>
  );
}

export function AgendaList({
  nodeKey,
  range,
  instances,
  calendars,
  tz,
  part = "calendar.agenda",
  label,
  empty,
}: Omit<DayViewProps, "anchor" | "onDay"> & {
  part?: string;
  label: string;
  /** What an empty range shows (the empty state). */
  empty: ReactNode;
}) {
  const t = useT();
  const format = useDateFormat();
  const now = useNow();
  const today = todayIn(tz, now);
  const days = agendaDays(instances, range, tz, today, now);
  if (days.length === 0) {
    return (
      <div data-part={part} data-state="empty" aria-label={label} role="region">
        {empty}
      </div>
    );
  }
  const nowMarker = (
    <li key="now" data-part={`${part}.now`}>
      <span data-part={`${part}.now.time`}>
        {t("pim-views-agenda-now", { time: formatClock(format, now, tz) })}
      </span>
    </li>
  );
  return (
    <div data-part={part} aria-label={label} role="region">
      {days.map(({ day, relative, rows, nowAt }) => {
        const date = formatDay(format, day, { weekday: "long", day: "numeric", month: "long" });
        const items = rows.map((row) => (
          <li key={`${row.instance.event_id}-${row.instance.start_utc}`} data-part={`${part}.row`}>
            <AgendaEntry
              row={row}
              part={part}
              geom={geomId(
                nodeKey,
                "a",
                dateString(day),
                row.instance.event_id,
                row.instance.start_utc,
              )}
              tz={tz}
              calendars={calendars}
            />
          </li>
        ));
        if (nowAt !== null) items.splice(nowAt, 0, nowMarker);
        return (
          <section
            key={dateString(day)}
            data-part={`${part}.day`}
            data-day={dateString(day)}
            data-state={relative ?? (day.compare(today) < 0 ? "past" : undefined)}
            aria-label={formatDay(format, day, { dateStyle: "full" })}
          >
            <h3 data-part={`${part}.day-head`}>
              {relative === "today"
                ? t("pim-today-with-date", { date })
                : relative === "tomorrow"
                  ? t("pim-views-tomorrow-with-date", { date })
                  : date}
            </h3>
            <ul data-part={`${part}.rows`}>{items}</ul>
          </section>
        );
      })}
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
  children?: ReactNode;
  onClick?: () => void;
  label?: string;
}) {
  useGeometry(geom, { left: percent(left), width: percent(Math.max(0.005, width)) });
  if (onClick) {
    return (
      <button type="button" data-part={part} data-geom={geom} onClick={onClick} aria-label={label}>
        {children}
      </button>
    );
  }
  return (
    <span data-part={part} data-geom={geom} title={label}>
      {children}
    </span>
  );
}

function Tick({ geom, at, children }: { geom: string; at: number; children: ReactNode }) {
  useGeometry(geom, { left: percent(at) });
  return (
    <span data-part="calendar.free-time.tick" data-geom={geom}>
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
  const ticks = hourTicks(dayStartHour, dayEndHour);
  const noTitle = t("common-no-title");

  return (
    <div data-part="calendar.free-time" aria-busy={free.isPending}>
      <p data-part="calendar.free-time.intro">{t("pim-free-time-intro", { minutes })}</p>
      <div data-part="calendar.free-time.axis" aria-hidden="true">
        {ticks.map((tick) => (
          <Tick key={tick.hour} geom={geomId(nodeKey, "tick", tick.hour)} at={tick.at}>
            {formatHour(format, tick.hour % 24)}
          </Tick>
        ))}
      </div>
      {workingDaysOf(range, locale).map((day) => {
        const midnight = toZoned(day, tz);
        const dayStart = midnight.add({ hours: dayStartHour }).toDate().getTime();
        const dayEnd = midnight.add({ hours: dayEndHour }).toDate().getTime();
        const position = (ms: number) => (ms - dayStart) / Math.max(1, dayEnd - dayStart);
        const slots = freeStretchesOfDay(
          free.data ?? [],
          day,
          tz,
          dayStartHour,
          dayEndHour,
          minutes,
        );
        const blocks = busyBlocks(instances, day, tz, dayStart, dayEnd);
        const allDay = busyAllDay(instances, day, tz);
        const total = Math.round(freeMinutes(slots));
        const dayLabel = formatDay(format, day, { weekday: "long", day: "numeric", month: "long" });
        return (
          <section key={dateString(day)} data-part="calendar.free-time.day" aria-label={dayLabel}>
            <div data-part="calendar.free-time.head">
              <h3 data-part="calendar.free-time.date">{dayLabel}</h3>
              {!free.isPending && (
                <span
                  data-part="calendar.free-time.total"
                  data-state={total === 0 ? "none" : undefined}
                >
                  {t("pim-views-free-total", {
                    hours: Math.floor(total / 60),
                    minutes: total % 60,
                  })}
                </span>
              )}
            </div>
            <div data-part="calendar.free-time.track">
              {blocks.map(({ instance, left, width }) => (
                <Segment
                  key={`${instance.event_id}-${instance.start_utc}`}
                  geom={geomId(nodeKey, "busy", instance.event_id, instance.start_utc)}
                  part="calendar.free-time.block"
                  left={left}
                  width={width}
                  label={t("pim-views-free-busy", {
                    title: instance.title || noTitle,
                    time: t("common-time-range", {
                      start: formatClock(format, instance.start_utc, tz),
                      end: formatClock(format, instance.end_utc, tz),
                    }),
                  })}
                >
                  {instance.title || noTitle}
                </Segment>
              ))}
              {slots.map((slot) => {
                const left = Math.max(0, position(instantMs(slot.start)));
                const right = Math.min(1, position(instantMs(slot.end)));
                const text = t("common-time-range", {
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
              <p data-part="calendar.free-time.none">
                {allDay.length > 0
                  ? t("pim-views-free-busy-all-day", {
                      titles: allDay.map((i) => i.title || noTitle).join(", "),
                    })
                  : t("pim-free-time-none")}
              </p>
            )}
          </section>
        );
      })}
    </div>
  );
}
