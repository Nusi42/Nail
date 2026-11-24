/**
 * The time grid of the day, multi-day, week and work-week views: day
 * headers, an all-day row (all-day and 24-hour-plus events spanning their
 * days, tasks due on each day), an hour gutter (and a second one for the
 * secondary zone), a grid of hour slots and the events of each day placed
 * by time, overlapping ones cascaded (`placeDay`).
 *
 * R7 (ADR 0246): today's date sits in a filled circle; the now line (2 px
 * with a dot on today, a faint line across the other days, the time in the
 * gutter) lies under the events; time outside the working hours is hatched
 * and days off are tinted (the calendar preferences); the week number shows
 * in the corner when switched on; the grid opens scrolled to the start of
 * the working day.
 *
 * Keyboard: the slot grid is one tab stop (arrows move between hours and
 * days, Shift + Up / Down extend the range, Enter draws a new event there
 * and asks for its title); events are buttons in time order (see
 * `EventItem`). Pointer: drag on empty slots to draw a new event in
 * 15-minute steps (a click takes the 15-minute slot under the pointer, a
 * double-click opens the editor there), drag an event to move it, its
 * bottom edge to resize it, drop a task to block time for it.
 *
 * Positions and sizes are data, so they go through geometry rules
 * (`useGeometry`), never inline styles.
 */
import type { CalendarDate } from "@internationalized/date";
import {
  useEffect,
  useRef,
  useState,
  type DragEvent,
  type KeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import type { Calendar, EventInstance, Task } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { useNow } from "../../state/clock";
import { useGeometry } from "../../theme/ThemeProvider";
import type { AnchorRect } from "../../widgets/Popover";
import { movedTimes, requestMove } from "./changes";
import { geomId, problemOf, useChangeContext } from "./common";
import { DueTaskItem, type DueTaskParts } from "./DueTaskItem";
import { EventItem, type DragMode } from "./EventItem";
import { dayDate, formatClock, formatDay, formatHour, formatInstant, zoneLabel } from "./fmt";
import { keepInitialScroll } from "./initialScroll";
import { lengthText } from "./eventLook";
import { NewEventDraft, useRangeText } from "./NewEventDraft";
import { NEW_EVENT_MINUTES, createEventIn, defaultEventCalendar, openNewEvent } from "./newEvent";
import {
  TASK_DRAG_TYPE,
  isoWeekNumber,
  isWorkDay,
  offHours,
  taskOfDrag,
  useCalendarPreferences,
  useTasksDue,
} from "./prefs";
import { rootFontPx, useElementSize } from "./viewHooks";
import { usePimStore } from "./store";
import {
  clickRange,
  dateString,
  dragRange,
  initialScrollTop,
  minuteAt,
  minuteOfDay,
  pimTimeIn,
  placeDay,
  placeSpanning,
  nowLabelCovers,
  rangeInstants,
  sideBySideMinutes,
  snap,
  toCalendarDate,
  todayIn,
  toZoned,
  visibleInstances,
  type MinuteRange,
} from "./time";
import { fromAbsolute } from "@internationalized/date";

export interface TimeGridProps {
  nodeKey: string;
  days: CalendarDate[];
  instances: readonly EventInstance[];
  calendars: readonly Calendar[] | undefined;
  tz: string;
  secondaryTz: string | null;
  /** First and last (exclusive) hour shown. */
  hourStart: number;
  hourEnd: number;
  /**
   * Hour to scroll to when shown; `null` or absent = the start of the
   * working day (the calendar preferences).
   */
  scrollTo?: number | null;
  /** Go to a day (day header). */
  onDay: (day: CalendarDate) => void;
}

interface DragState {
  instance: EventInstance;
  mode: DragMode;
  dayIndex: number;
  startX: number;
  startY: number;
  minutesPerPixel: number;
  columnWidth: number;
  days: number;
  minutes: number;
  moved: boolean;
}

/** A range being drawn on empty slots (pointer held). */
interface CreateState {
  dayIndex: number;
  anchor: number;
  startY: number;
  range: MinuteRange;
  moved: boolean;
}

/** A drawn range waiting for its title. */
interface DraftState {
  dayIndex: number;
  range: MinuteRange;
  anchor: AnchorRect;
}

/** Pointer travel (CSS pixels) after which a press on a slot draws a range instead of clicking. */
const DRAW_THRESHOLD = 4;

const pct = (value: number) => `${String(Math.round(value * 10000) / 100)}%`;

function Positioned({
  geom,
  part,
  declarations,
  state,
  children,
}: {
  geom: string;
  part: string;
  declarations: Record<string, string>;
  state?: string;
  children?: ReactNode;
}) {
  useGeometry(geom, declarations);
  return (
    <div data-part={part} data-geom={geom} data-state={state} aria-hidden="true">
      {children}
    </div>
  );
}

function Columns({
  geom,
  part,
  count,
  children,
  rows,
}: {
  geom: string;
  part: string;
  count: number;
  rows?: number;
  children: React.ReactNode;
}) {
  useGeometry(geom, {
    "grid-template-columns": `repeat(${String(count)}, minmax(0, 1fr))`,
    ...(rows !== undefined ? { "grid-template-rows": `repeat(${String(rows)}, auto)` } : {}),
  });
  return (
    <div data-part={part} data-geom={geom}>
      {children}
    </div>
  );
}

/** Tasks due on each day of `days` (the day of their due time in `tz`). */
export function tasksByDay(
  tasks: readonly Task[],
  days: readonly CalendarDate[],
  tz: string,
): Task[][] {
  const index = new Map(days.map((day, i) => [dateString(day), i]));
  const out: Task[][] = days.map(() => []);
  for (const task of tasks) {
    const due = task.fields.due;
    if (due === null) continue;
    const i = index.get(dateString(toCalendarDate(pimTimeIn(due, tz))));
    if (i !== undefined) out[i]?.push(task);
  }
  return out;
}

/** The wall-clock time `minutes` after midnight. */
function minuteTime(minutes: number): { hour: number; minute: number } {
  return { hour: Math.floor(minutes / 60), minute: minutes % 60 };
}

export function TimeGrid({
  nodeKey,
  days,
  instances,
  calendars,
  tz,
  secondaryTz,
  hourStart,
  hourEnd,
  scrollTo = null,
  onDay,
}: TimeGridProps) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useChangeContext();
  const pim = usePimStore();
  const prefs = useCalendarPreferences();
  const rangeText = useRangeText();
  const nowMs = useNow();
  const today = todayIn(tz, nowMs);
  const hours = Array.from({ length: Math.max(1, hourEnd - hourStart) }, (_, i) => hourStart + i);
  const firstDay = days[0];
  const visibleMinutes = hours.length * 60;
  const fromMinute = hourStart * 60;
  const toMinute = fromMinute + visibleMinutes;
  const scrollHour = scrollTo ?? Math.floor(prefs.work_hours.start_minute / 60);
  const shown = visibleInstances(instances, prefs.show_declined);
  const scrollRef = useRef<HTMLDivElement>(null);
  const columnsRef = useRef<HTMLDivElement>(null);
  const slotsRef = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<DragState | null>(null);
  const [create, setCreate] = useState<CreateState | null>(null);
  const [draft, setDraft] = useState<DraftState | null>(null);
  const justDragged = useRef(false);
  const [chosenSlot, setSlot] = useState<{ day: number; hour: number; extent: number | null }>(
    () => ({
      day: Math.max(
        0,
        days.findIndex((d) => d.compare(today) === 0),
      ),
      hour: Math.min(Math.max(scrollHour, hourStart), hourEnd - 1),
      extent: null,
    }),
  );
  // The view may have fewer days or hours than when the slot was chosen.
  const clampHour = (hour: number) => Math.min(Math.max(hour, hourStart), hourEnd - 1);
  const slot = {
    day: Math.min(Math.max(0, chosenSlot.day), days.length - 1),
    hour: clampHour(chosenSlot.hour),
    extent: chosenSlot.extent === null ? null : clampHour(chosenSlot.extent),
  };
  const slotHours = {
    from: Math.min(slot.hour, slot.extent ?? slot.hour),
    to: Math.max(slot.hour, slot.extent ?? slot.hour),
  };
  const slotKeyboard = useRef(false);
  // A due task's Plan goes to the slot chosen here (R7 S12).
  const slotDay = days[slot.day];
  const planDay = slotDay ? dateString(slotDay) : null;
  const planMinute = slot.hour * 60;
  useEffect(() => {
    if (planDay !== null) pim.getState().setPlanSlot({ day: planDay, minute: planMinute });
  }, [pim, planDay, planMinute]);

  // Only when the view is shown or its hours change, not on every render
  // (and again while the layout settles, until the user scrolls).
  const hourCount = hours.length;
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    return keepInitialScroll(el, () =>
      initialScrollTop(
        scrollHour - hourStart,
        el.scrollHeight / Math.max(1, hourCount),
        el.scrollHeight - el.clientHeight,
      ),
    );
  }, [scrollHour, hourStart, hourCount]);

  useEffect(() => {
    if (!slotKeyboard.current) return;
    slotKeyboard.current = false;
    slotsRef.current
      ?.querySelector<HTMLElement>(
        `[data-slot="${String(chosenSlot.day)}-${String(chosenSlot.hour)}"]`,
      )
      ?.focus();
  }, [chosenSlot]);

  const repeat = { "grid-template-columns": `repeat(${String(days.length)}, minmax(0, 1fr))` };
  const slotsGeom = geomId(nodeKey, "slots");
  const columnsGeom = geomId(nodeKey, "columns");
  useGeometry(slotsGeom, repeat);
  useGeometry(columnsGeom, repeat);
  // Side by side or cascaded: from the height a cascade leaves visible.
  const columnsHeight = useElementSize(columnsRef).height;
  const together = sideBySideMinutes(columnsHeight / visibleMinutes, rootFontPx());

  const zones = secondaryTz ? 2 : 1;
  const spanning = placeSpanning(shown, days, tz);
  const spanRows = spanning.length === 0 ? 0 : Math.max(...spanning.map((s) => s.row + 1));
  const tasksDue = useTasksDue(
    firstDay ? rangeInstants({ start: firstDay, count: days.length }, tz) : null,
    null,
    tz,
  );
  const dueByDay = tasksByDay(tasksDue, days, tz);
  const rows = Math.max(1, spanRows + Math.max(0, ...dueByDay.map((list) => list.length)));
  const defaultCalendar = defaultEventCalendar(calendars ?? []);

  // ---- Where things are on screen ----
  const box = () => {
    const rect = columnsRef.current?.getBoundingClientRect();
    return rect && rect.width > 0 && rect.height > 0 ? rect : null;
  };
  /** The day and minute under a viewport point, `null` outside the grid. */
  const pointAt = (x: number, y: number): { dayIndex: number; minute: number } | null => {
    const rect = box();
    if (!rect) return null;
    const dayIndex = Math.floor(((x - rect.left) / rect.width) * days.length);
    if (dayIndex < 0 || dayIndex >= days.length) return null;
    return { dayIndex, minute: minuteAt(y - rect.top, rect.height, fromMinute, toMinute) };
  };
  /** Where a range of a day is on screen (the inline title's anchor). */
  const anchorFor = (dayIndex: number, range: MinuteRange): AnchorRect => {
    const rect = columnsRef.current?.getBoundingClientRect();
    const width = (rect?.width ?? 0) / Math.max(1, days.length);
    const height = rect?.height ?? 0;
    const y = (minute: number) => ((minute - fromMinute) / visibleMinutes) * height;
    return {
      left: (rect?.left ?? 0) + dayIndex * width,
      top: (rect?.top ?? 0) + y(range.start),
      width,
      height: y(range.end) - y(range.start),
    };
  };

  const showDraft = (dayIndex: number, range: MinuteRange) => {
    setDraft({ dayIndex, range, anchor: anchorFor(dayIndex, range) });
  };

  const onSlotKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    let next: { day: number; hour: number; extent: number | null };
    const extending = event.shiftKey && (event.key === "ArrowUp" || event.key === "ArrowDown");
    const end = slot.extent ?? slot.hour;
    switch (event.key) {
      case "ArrowUp":
        next = extending
          ? { ...slot, extent: Math.max(hourStart, end - 1) }
          : { ...slot, hour: Math.max(hourStart, slot.hour - 1), extent: null };
        break;
      case "ArrowDown":
        next = extending
          ? { ...slot, extent: Math.min(hourEnd - 1, end + 1) }
          : { ...slot, hour: Math.min(hourEnd - 1, slot.hour + 1), extent: null };
        break;
      case "ArrowLeft":
        next = { ...slot, day: Math.max(0, slot.day - 1), extent: null };
        break;
      case "ArrowRight":
        next = { ...slot, day: Math.min(days.length - 1, slot.day + 1), extent: null };
        break;
      case "Home":
        next = { ...slot, hour: hourStart, extent: null };
        break;
      case "End":
        next = { ...slot, hour: hourEnd - 1, extent: null };
        break;
      case "Enter":
      case " ": {
        event.preventDefault();
        showDraft(slot.day, { start: slotHours.from * 60, end: (slotHours.to + 1) * 60 });
        return;
      }
      default:
        return;
    }
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    event.preventDefault();
    if (!extending) slotKeyboard.current = true;
    setSlot(next);
  };

  // ---- Drawing a new event ----
  const startCreate = (event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    const at = pointAt(event.clientX, event.clientY);
    if (!at) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    setDraft(null);
    setCreate({
      dayIndex: at.dayIndex,
      anchor: at.minute,
      startY: event.clientY,
      range: dragRange(at.minute, at.minute, fromMinute, toMinute),
      moved: false,
    });
  };

  // ---- Dragging ----
  const startDrag = (
    event: ReactPointerEvent<HTMLElement>,
    instance: EventInstance,
    dayIndex: number,
    mode: DragMode,
  ) => {
    if (event.button !== 0) return;
    const columns = box();
    if (!columns) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    setDrag({
      instance,
      mode,
      dayIndex,
      startX: event.clientX,
      startY: event.clientY,
      minutesPerPixel: visibleMinutes / columns.height,
      columnWidth: columns.width / Math.max(1, days.length),
      days: 0,
      minutes: 0,
      moved: false,
    });
  };

  const onPointerMove = (event: ReactPointerEvent<HTMLElement>) => {
    if (create) {
      const rect = box();
      if (!rect) return;
      if (!create.moved && Math.abs(event.clientY - create.startY) <= DRAW_THRESHOLD) return;
      const minute = minuteAt(event.clientY - rect.top, rect.height, fromMinute, toMinute);
      const range = dragRange(create.anchor, minute, fromMinute, toMinute);
      if (!create.moved || range.start !== create.range.start || range.end !== create.range.end) {
        setCreate({ ...create, range, moved: true });
      }
      return;
    }
    if (!drag) return;
    const minutes = snap((event.clientY - drag.startY) * drag.minutesPerPixel);
    const dayShift =
      drag.mode === "move"
        ? Math.max(
            -drag.dayIndex,
            Math.min(
              days.length - 1 - drag.dayIndex,
              Math.round((event.clientX - drag.startX) / drag.columnWidth),
            ),
          )
        : 0;
    if (minutes !== drag.minutes || dayShift !== drag.days) {
      setDrag({ ...drag, minutes, days: dayShift, moved: true });
    }
  };

  const onPointerUp = () => {
    if (create) {
      const finished = create;
      setCreate(null);
      showDraft(
        finished.dayIndex,
        finished.moved
          ? finished.range
          : clickRange(finished.anchor, NEW_EVENT_MINUTES, fromMinute, toMinute),
      );
      return;
    }
    if (!drag) return;
    const finished = drag;
    setDrag(null);
    if (!finished.moved || (finished.minutes === 0 && finished.days === 0)) return;
    justDragged.current = true;
    const { start, end } = movedTimes(
      finished.instance,
      finished.days,
      finished.minutes,
      tz,
      finished.mode === "resize",
    );
    void requestMove(ctx, finished.instance, start, end).catch(() => {
      pim.getState().pushToast({
        kind: "problem",
        id: `move-${String(Date.now())}`,
        message: { id: "pim-move-failed", args: { error: "" } },
      });
    });
  };

  const suppressClick = () => {
    if (!justDragged.current) return false;
    justDragged.current = false;
    return true;
  };

  // ---- Dropping a task (S12) ----
  const onDragOver = (event: DragEvent<HTMLElement>) => {
    if (!event.dataTransfer.types.includes(TASK_DRAG_TYPE)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "copy";
  };
  const onDrop = (event: DragEvent<HTMLElement>) => {
    const task = taskOfDrag(event.dataTransfer);
    const at = pointAt(event.clientX, event.clientY);
    const day = at ? days[at.dayIndex] : undefined;
    if (!task || !at || !day || !defaultCalendar) return;
    event.preventDefault();
    const range = clickRange(at.minute, NEW_EVENT_MINUTES, fromMinute, toMinute);
    createEventIn(ctx, defaultCalendar, day, range, tz, task.title).catch((error: unknown) => {
      pim.getState().pushToast({
        kind: "problem",
        id: `task-drop-${String(Date.now())}`,
        message: problemOf(error),
      });
    });
  };

  const ghost = (() => {
    if (!drag?.moved) return null;
    const dragDay = days[drag.dayIndex];
    if (!dragDay) return null;
    const placed = placeDay([drag.instance], dragDay, tz)[0];
    if (!placed) return null;
    const start = drag.mode === "move" ? placed.startMinute + drag.minutes : placed.startMinute;
    const end =
      drag.mode === "move"
        ? placed.endMinute + drag.minutes
        : Math.max(placed.startMinute + 15, placed.endMinute + drag.minutes);
    const top = (start - fromMinute) / visibleMinutes;
    const height = (end - start) / visibleMinutes;
    return {
      left: (drag.dayIndex + drag.days) / days.length,
      width: 1 / days.length,
      top,
      height,
    };
  })();

  /** The range being drawn or waiting for its title. */
  const drawn = create?.moved
    ? { dayIndex: create.dayIndex, range: create.range }
    : draft
      ? { dayIndex: draft.dayIndex, range: draft.range }
      : null;
  const draftDay = draft ? days[draft.dayIndex] : undefined;

  const nowWall = minuteOfDay(fromAbsolute(nowMs, tz));
  const todayIndex = days.findIndex((d) => d.compare(today) === 0);
  const showNow = todayIndex >= 0 && nowWall >= fromMinute && nowWall < toMinute;
  const nowTop = pct((nowWall - fromMinute) / visibleMinutes);
  const dayOff = (day: CalendarDate) => !isWorkDay(prefs, day);
  const columnOf = (dayIndex: number) => ({
    left: pct(dayIndex / days.length),
    width: pct(1 / days.length),
  });

  return (
    <div
      data-part="calendar.time-grid"
      data-days={days.length}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={() => {
        setDrag(null);
        setCreate(null);
      }}
    >
      <div data-part="calendar.head" data-zones={zones}>
        <div data-part="calendar.corner">
          {prefs.show_week_numbers && firstDay && (
            <span data-part="calendar.week-number">
              {t("pim-grid-week-number", { week: isoWeekNumber(firstDay) })}
            </span>
          )}
          <span data-part="calendar.zone">{zoneLabel(format, nowMs, tz)}</span>
          {secondaryTz && (
            <span data-part="calendar.zone" data-zone="secondary">
              {zoneLabel(format, nowMs, secondaryTz)}
            </span>
          )}
        </div>
        <Columns geom={geomId(nodeKey, "head")} part="calendar.head.days" count={days.length}>
          {days.map((day) => (
            <button
              key={dateString(day)}
              type="button"
              data-part="calendar.day-head"
              data-day={dateString(day)}
              data-state={
                [day.compare(today) === 0 ? "today" : null, dayOff(day) ? "day-off" : null]
                  .filter(Boolean)
                  .join(" ") || undefined
              }
              aria-current={day.compare(today) === 0 ? "date" : undefined}
              aria-label={t("pim-go-to-day", {
                day: formatDay(format, day, { dateStyle: "full" }),
              })}
              onClick={() => {
                onDay(day);
              }}
            >
              <span data-part="calendar.day-head.weekday">
                {format.dateTime(dayDate(day), { weekday: "short", timeZone: "UTC" })}
              </span>
              <span data-part="calendar.day-head.date">
                {format.dateTime(dayDate(day), { day: "numeric", timeZone: "UTC" })}
              </span>
            </button>
          ))}
        </Columns>
      </div>

      <div data-part="calendar.all-day" data-zones={zones}>
        <span data-part="calendar.all-day.label">{t("common-all-day")}</span>
        <Columns
          geom={geomId(nodeKey, "all-day")}
          part="calendar.all-day.days"
          count={days.length}
          rows={rows}
        >
          {days.map((day, index) => (
            <AllDayCell
              key={dateString(day)}
              geom={geomId(nodeKey, "all-day-cell", index)}
              index={index}
              rows={rows}
              day={day}
              label={t("pim-new-all-day-event", {
                day: formatDay(format, day, { dateStyle: "full" }),
              })}
              onCreate={() => {
                openNewEvent(ctx, day, null, tz);
              }}
            />
          ))}
          <ul data-part="calendar.all-day.events" aria-label={t("pim-all-day-events")}>
            {spanning.map((s) => (
              <li
                key={`${s.instance.event_id}-${s.instance.start_utc}`}
                data-part="calendar.all-day.item"
              >
                <EventItem
                  instance={s.instance}
                  tz={tz}
                  calendars={calendars}
                  geom={geomId(nodeKey, "span", s.instance.event_id, s.instance.start_utc)}
                  position={{
                    "grid-column": `${String(s.first + 1)} / span ${String(s.last - s.first + 1)}`,
                    "grid-row": String(s.row + 1),
                  }}
                  part="calendar.chip"
                  keys="days"
                  states={[
                    s.continuesBefore ? "continues-before" : null,
                    s.continuesAfter ? "continues-after" : null,
                  ].filter((x): x is string => x !== null)}
                />
              </li>
            ))}
          </ul>
          {tasksDue.length > 0 && (
            <ul data-part="calendar.tasks" aria-label={t("pim-grid-tasks-due")}>
              {dueByDay.flatMap((list, dayIndex) =>
                list.map((task, i) => (
                  <DueTask
                    key={task.id}
                    task={task}
                    geom={geomId(nodeKey, "task", task.id)}
                    column={dayIndex + 1}
                    row={spanRows + i + 1}
                    tz={tz}
                  />
                )),
              )}
            </ul>
          )}
        </Columns>
      </div>

      <div data-part="calendar.scroll" ref={scrollRef}>
        <div data-part="calendar.body" data-zones={zones}>
          <div data-part="calendar.gutter" aria-hidden="true">
            {hours.map((hour) => (
              <span
                key={hour}
                data-part="calendar.hour"
                // The now label takes the place of an hour label it would cover.
                data-state={showNow && nowLabelCovers(hour, nowWall) ? "near-now" : undefined}
              >
                {formatHour(format, hour)}
              </span>
            ))}
            {showNow && (
              <Positioned
                geom={geomId(nodeKey, "now-label")}
                part="calendar.now-label"
                declarations={{ top: nowTop }}
              >
                {formatClock(format, nowMs, tz)}
              </Positioned>
            )}
          </div>
          {secondaryTz && firstDay && (
            <div data-part="calendar.gutter" data-zone="secondary" aria-hidden="true">
              {hours.map((hour) => {
                const ms = toZoned(firstDay, tz).add({ hours: hour }).toDate().getTime();
                return (
                  <span key={hour} data-part="calendar.hour">
                    {formatInstant(format, ms, secondaryTz, { hour: "numeric" })}
                  </span>
                );
              })}
            </div>
          )}
          <div data-part="calendar.canvas" onDragOver={onDragOver} onDrop={onDrop}>
            {days.map((day, dayIndex) =>
              dayOff(day) ? (
                <Positioned
                  key={dateString(day)}
                  geom={geomId(nodeKey, "off", dayIndex)}
                  part="calendar.off-hours"
                  state="day-off"
                  declarations={{ ...columnOf(dayIndex), top: "0", height: "100%" }}
                />
              ) : (
                offHours(prefs, day).map(([a, b]) => {
                  const start = Math.max(a, fromMinute);
                  const end = Math.min(b, toMinute);
                  if (end <= start) return null;
                  return (
                    <Positioned
                      key={`${dateString(day)}-${String(a)}`}
                      geom={geomId(nodeKey, "off", dayIndex, a)}
                      part="calendar.off-hours"
                      declarations={{
                        ...columnOf(dayIndex),
                        top: pct((start - fromMinute) / visibleMinutes),
                        height: pct((end - start) / visibleMinutes),
                      }}
                    />
                  );
                })
              ),
            )}
            {/* eslint-disable-next-line jsx-a11y/interactive-supports-focus -- the grid's cells take the focus (roving tabindex), not the grid itself. */}
            <div
              ref={slotsRef}
              role="grid"
              data-part="calendar.slots"
              data-geom={slotsGeom}
              aria-label={t("pim-slots-label")}
              aria-multiselectable="true"
              onKeyDown={onSlotKeyDown}
              onPointerDown={startCreate}
            >
              {hours.map((hour) => (
                <div key={hour} role="row" data-part="calendar.slot-row">
                  {days.map((day, dayIndex) => {
                    const active = slot.day === dayIndex && slot.hour === hour;
                    const inRange =
                      slot.extent !== null &&
                      slot.day === dayIndex &&
                      hour >= slotHours.from &&
                      hour <= slotHours.to;
                    return (
                      <div
                        key={dateString(day)}
                        role="gridcell"
                        data-part="calendar.slot"
                        data-slot={`${String(dayIndex)}-${String(hour)}`}
                        data-day={dateString(day)}
                        aria-selected={inRange}
                        tabIndex={active ? 0 : -1}
                        aria-label={t("pim-slot-label", {
                          day: formatDay(format, day, {
                            weekday: "long",
                            day: "numeric",
                            month: "long",
                          }),
                          time: formatHour(format, hour),
                        })}
                        onFocus={() => {
                          if (!active) setSlot({ day: dayIndex, hour, extent: null });
                        }}
                        onDoubleClick={(event) => {
                          setDraft(null);
                          const at = pointAt(event.clientX, event.clientY);
                          const start =
                            at?.dayIndex === dayIndex
                              ? clickRange(at.minute, NEW_EVENT_MINUTES, fromMinute, toMinute).start
                              : hour * 60;
                          openNewEvent(ctx, day, minuteTime(start), tz);
                        }}
                      />
                    );
                  })}
                </div>
              ))}
            </div>
            {slot.extent !== null && (
              <Positioned
                geom={geomId(nodeKey, "slot-range")}
                part="calendar.slot-range"
                declarations={{
                  ...columnOf(slot.day),
                  top: pct((slotHours.from * 60 - fromMinute) / visibleMinutes),
                  height: pct(((slotHours.to - slotHours.from + 1) * 60) / visibleMinutes),
                }}
              />
            )}
            {showNow && (
              <>
                <Positioned
                  geom={geomId(nodeKey, "now-week")}
                  part="calendar.now"
                  state="week"
                  declarations={{ left: "0", width: "100%", top: nowTop }}
                />
                <Positioned
                  geom={geomId(nodeKey, "now")}
                  part="calendar.now"
                  state="today"
                  declarations={{ ...columnOf(todayIndex), top: nowTop }}
                />
              </>
            )}
            <div ref={columnsRef} data-part="calendar.columns" data-geom={columnsGeom}>
              {days.map((day, dayIndex) => {
                const placed = placeDay(shown, day, tz, together);
                return (
                  <ul
                    key={dateString(day)}
                    data-part="calendar.column"
                    data-day={dateString(day)}
                    aria-label={t("pim-events-of-day", {
                      day: formatDay(format, day, {
                        weekday: "long",
                        day: "numeric",
                        month: "long",
                      }),
                      count: placed.length,
                    })}
                  >
                    {placed.map((p) => {
                      const top = (p.startMinute - fromMinute) / visibleMinutes;
                      const height = (p.endMinute - p.startMinute) / visibleMinutes;
                      const dragging =
                        drag?.moved === true &&
                        drag.instance.event_id === p.instance.event_id &&
                        drag.instance.start_utc === p.instance.start_utc;
                      return (
                        <li
                          key={`${p.instance.event_id}-${p.instance.start_utc}`}
                          data-part="calendar.column.item"
                        >
                          <EventItem
                            instance={p.instance}
                            tz={tz}
                            calendars={calendars}
                            geom={geomId(
                              nodeKey,
                              "ev",
                              dateString(day),
                              p.instance.event_id,
                              p.instance.start_utc,
                            )}
                            position={{
                              top: pct(top),
                              height: pct(height),
                              left: pct(p.left),
                              width: pct(p.width),
                              "z-index": String(p.layer + 1),
                            }}
                            part="calendar.event"
                            keys="time"
                            short={p.short}
                            states={[
                              p.continuesBefore ? "continues-before" : null,
                              p.continuesAfter ? "continues-after" : null,
                              p.layer > 0 ? "stacked" : null,
                              dragging ? "dragging" : null,
                            ].filter((x): x is string => x !== null)}
                            onDragStart={(event, mode) => {
                              startDrag(event, p.instance, dayIndex, mode);
                            }}
                            suppressClick={suppressClick}
                          />
                        </li>
                      );
                    })}
                  </ul>
                );
              })}
              {ghost && (
                <Positioned
                  geom={geomId(nodeKey, "ghost")}
                  part="calendar.ghost"
                  declarations={{
                    left: pct(ghost.left),
                    width: pct(ghost.width),
                    top: pct(ghost.top),
                    height: pct(ghost.height),
                  }}
                />
              )}
              {drawn && (
                <Positioned
                  geom={geomId(nodeKey, "draft")}
                  part="calendar.draft"
                  declarations={{
                    ...columnOf(drawn.dayIndex),
                    top: pct((drawn.range.start - fromMinute) / visibleMinutes),
                    height: pct((drawn.range.end - drawn.range.start) / visibleMinutes),
                  }}
                >
                  <span data-part="calendar.draft.time">
                    {t("pim-grid-draft-time", {
                      time: rangeText(drawn.range),
                      length: lengthText(t, format.locale, drawn.range.end - drawn.range.start),
                    })}
                  </span>
                </Positioned>
              )}
            </div>
          </div>
        </div>
      </div>
      {draft && draftDay && (
        <NewEventDraft
          anchor={draft.anchor}
          day={draftDay}
          range={draft.range}
          tz={tz}
          calendars={calendars}
          onClose={() => {
            setDraft(null);
            // A keyboard range ends with its draft (saved or cancelled).
            setSlot((s) => (s.extent === null ? s : { ...s, extent: null }));
          }}
        />
      )}
    </div>
  );
}

/** The parts of a task due on a day of the view, in the all-day row (R7 S12). */
const ALL_DAY_TASK_PARTS: DueTaskParts = {
  item: "calendar.task",
  check: "calendar.task.check",
  title: "calendar.task.title",
  plan: "calendar.task.plan",
};

/** A task due on a day of the view, in the all-day row (R7 S12). */
function DueTask({
  task,
  geom,
  column,
  row,
  tz,
}: {
  task: Task;
  geom: string;
  column: number;
  row: number;
  tz: string;
}) {
  useGeometry(geom, { "grid-column": String(column), "grid-row": String(row) });
  return <DueTaskItem task={task} parts={ALL_DAY_TASK_PARTS} tz={tz} geom={geom} />;
}

function AllDayCell({
  geom,
  index,
  rows,
  day,
  label,
  onCreate,
}: {
  geom: string;
  index: number;
  rows: number;
  day: CalendarDate;
  label: string;
  onCreate: () => void;
}) {
  useGeometry(geom, {
    "grid-column": String(index + 1),
    "grid-row": `1 / span ${String(rows)}`,
  });
  return (
    <button
      type="button"
      data-part="calendar.all-day.cell"
      data-geom={geom}
      data-day={dateString(day)}
      aria-label={label}
      tabIndex={-1}
      onDoubleClick={onCreate}
      onClick={(event) => {
        if (event.detail === 0) onCreate();
      }}
    />
  );
}
