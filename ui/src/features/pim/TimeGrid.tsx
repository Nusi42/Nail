/**
 * The time grid of the day, multi-day, week and work-week views: day
 * headers, an all-day row (all-day and 24-hour-plus events spanning their
 * days), an hour gutter (and a second one for the secondary zone), a grid
 * of hour slots and the events of each day placed by time, overlapping ones
 * side by side.
 *
 * Keyboard: the slot grid is one tab stop (arrows move between hours and
 * days, Enter creates an event there); events are buttons in time order
 * (see `EventItem` for moving them). Pointer: drag an event to move it, its
 * bottom edge to resize it, double-click a slot to create an event.
 *
 * Positions and sizes are data, so they go through geometry rules
 * (`useGeometry`), never inline styles.
 */
import type { CalendarDate } from "@internationalized/date";
import {
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import type { Calendar, EventInstance } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { useNow } from "../../state/clock";
import { useGeometry } from "../../theme/ThemeProvider";
import { movedTimes, requestMove } from "./changes";
import { geomId, useChangeContext } from "./common";
import { EventItem, type DragMode } from "./EventItem";
import { dayDate, formatDay, formatHour, formatInstant, zoneLabel } from "./fmt";
import { usePimStore } from "./store";
import { openNewEvent } from "./newEvent";
import {
  dateString,
  initialScrollTop,
  minuteOfDay,
  placeDay,
  placeSpanning,
  snap,
  todayIn,
  toZoned,
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
  /** Hour to scroll to when shown. */
  scrollTo: number;
  /** Go to a day (day header). */
  onDay(day: CalendarDate): void;
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

const pct = (value: number) => `${Math.round(value * 10000) / 100}%`;

function Positioned({
  geom,
  part,
  declarations,
}: {
  geom: string;
  part: string;
  declarations: Record<string, string>;
}) {
  useGeometry(geom, declarations);
  return <div data-part={part} data-geom={geom} aria-hidden="true" />;
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
    "grid-template-columns": `repeat(${count}, minmax(0, 1fr))`,
    ...(rows !== undefined ? { "grid-template-rows": `repeat(${rows}, auto)` } : {}),
  });
  return (
    <div data-part={part} data-geom={geom}>
      {children}
    </div>
  );
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
  scrollTo,
  onDay,
}: TimeGridProps) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useChangeContext();
  const pim = usePimStore();
  const nowMs = useNow();
  const today = todayIn(tz, nowMs);
  const hours = Array.from({ length: Math.max(1, hourEnd - hourStart) }, (_, i) => hourStart + i);
  const visibleMinutes = hours.length * 60;
  const scrollRef = useRef<HTMLDivElement>(null);
  const columnsRef = useRef<HTMLDivElement>(null);
  const slotsRef = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<DragState | null>(null);
  const justDragged = useRef(false);
  const [chosenSlot, setSlot] = useState<{ day: number; hour: number }>(() => ({
    day: Math.max(
      0,
      days.findIndex((d) => d.compare(today) === 0),
    ),
    hour: Math.min(Math.max(scrollTo, hourStart), hourEnd - 1),
  }));
  // The view may have fewer days or hours than when the slot was chosen.
  const slot = {
    day: Math.min(Math.max(0, chosenSlot.day), days.length - 1),
    hour: Math.min(Math.max(chosenSlot.hour, hourStart), hourEnd - 1),
  };
  const slotKeyboard = useRef(false);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = initialScrollTop(
      scrollTo - hourStart,
      el.scrollHeight / Math.max(1, hours.length),
      el.scrollHeight - el.clientHeight,
    );
    // Only when the view is shown, not on every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [scrollTo, hourStart]);

  useEffect(() => {
    if (!slotKeyboard.current) return;
    slotKeyboard.current = false;
    slotsRef.current
      ?.querySelector<HTMLElement>(`[data-slot="${chosenSlot.day}-${chosenSlot.hour}"]`)
      ?.focus();
  }, [chosenSlot]);

  const repeat = { "grid-template-columns": `repeat(${days.length}, minmax(0, 1fr))` };
  const slotsGeom = geomId(nodeKey, "slots");
  const columnsGeom = geomId(nodeKey, "columns");
  useGeometry(slotsGeom, repeat);
  useGeometry(columnsGeom, repeat);

  const zones = secondaryTz ? 2 : 1;
  const spanning = placeSpanning(instances, days, tz);
  const rows = Math.max(1, ...spanning.map((s) => s.row + 1));

  const createAt = (day: CalendarDate, hour: number | null) => {
    openNewEvent(ctx, day, hour === null ? null : { hour, minute: 0 }, tz);
  };

  const onSlotKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    let next: { day: number; hour: number };
    switch (event.key) {
      case "ArrowUp":
        next = { ...slot, hour: Math.max(hourStart, slot.hour - 1) };
        break;
      case "ArrowDown":
        next = { ...slot, hour: Math.min(hourEnd - 1, slot.hour + 1) };
        break;
      case "ArrowLeft":
        next = { ...slot, day: Math.max(0, slot.day - 1) };
        break;
      case "ArrowRight":
        next = { ...slot, day: Math.min(days.length - 1, slot.day + 1) };
        break;
      case "Home":
        next = { ...slot, hour: hourStart };
        break;
      case "End":
        next = { ...slot, hour: hourEnd - 1 };
        break;
      case "Enter":
      case " ": {
        event.preventDefault();
        const day = days[slot.day];
        if (day) createAt(day, slot.hour);
        return;
      }
      default:
        return;
    }
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    event.preventDefault();
    slotKeyboard.current = true;
    setSlot(next);
  };

  // ---- Dragging ----
  const startDrag = (
    event: ReactPointerEvent<HTMLElement>,
    instance: EventInstance,
    dayIndex: number,
    mode: DragMode,
  ) => {
    if (event.button !== 0) return;
    const columns = columnsRef.current?.getBoundingClientRect();
    if (!columns || columns.height <= 0 || columns.width <= 0) return;
    event.currentTarget.setPointerCapture?.(event.pointerId);
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
        id: `move-${Date.now()}`,
        message: { id: "pim-move-failed", args: { error: "" } },
      });
    });
  };

  const suppressClick = () => {
    if (!justDragged.current) return false;
    justDragged.current = false;
    return true;
  };

  const ghost = (() => {
    if (!drag || !drag.moved) return null;
    const placed = placeDay([drag.instance], days[drag.dayIndex] as CalendarDate, tz)[0];
    if (!placed) return null;
    const start = drag.mode === "move" ? placed.startMinute + drag.minutes : placed.startMinute;
    const end =
      drag.mode === "move"
        ? placed.endMinute + drag.minutes
        : Math.max(placed.startMinute + 15, placed.endMinute + drag.minutes);
    const top = (start - hourStart * 60) / visibleMinutes;
    const height = (end - start) / visibleMinutes;
    return {
      left: (drag.dayIndex + drag.days) / days.length,
      width: 1 / days.length,
      top,
      height,
    };
  })();

  const nowWall = minuteOfDay(fromAbsolute(nowMs, tz));
  const todayIndex = days.findIndex((d) => d.compare(today) === 0);

  return (
    <div
      data-part="calendar.time-grid"
      data-days={days.length}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={() => setDrag(null)}
    >
      <div data-part="calendar.head" data-zones={zones}>
        <div data-part="calendar.corner">
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
              data-state={day.compare(today) === 0 ? "today" : undefined}
              aria-current={day.compare(today) === 0 ? "date" : undefined}
              aria-label={t("pim-go-to-day", {
                day: formatDay(format, day, { dateStyle: "full" }),
              })}
              onClick={() => onDay(day)}
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
        <span data-part="calendar.all-day.label">{t("pim-all-day")}</span>
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
              onCreate={() => createAt(day, null)}
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
                    "grid-column": `${s.first + 1} / span ${s.last - s.first + 1}`,
                    "grid-row": `${s.row + 1}`,
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
        </Columns>
      </div>

      <div data-part="calendar.scroll" ref={scrollRef}>
        <div data-part="calendar.body" data-zones={zones}>
          <div data-part="calendar.gutter" aria-hidden="true">
            {hours.map((hour) => (
              <span key={hour} data-part="calendar.hour">
                {formatHour(format, hour)}
              </span>
            ))}
          </div>
          {secondaryTz && (
            <div data-part="calendar.gutter" data-zone="secondary" aria-hidden="true">
              {hours.map((hour) => {
                const day = days[0] as CalendarDate;
                const ms = toZoned(day, tz).add({ hours: hour }).toDate().getTime();
                return (
                  <span key={hour} data-part="calendar.hour">
                    {formatInstant(format, ms, secondaryTz, { hour: "numeric" })}
                  </span>
                );
              })}
            </div>
          )}
          <div data-part="calendar.canvas">
            <div
              ref={slotsRef}
              role="grid"
              data-part="calendar.slots"
              data-geom={slotsGeom}
              aria-label={t("pim-slots-label")}
              onKeyDown={onSlotKeyDown}
            >
              {hours.map((hour) => (
                <div key={hour} role="row" data-part="calendar.slot-row">
                  {days.map((day, dayIndex) => {
                    const active = slot.day === dayIndex && slot.hour === hour;
                    return (
                      <div
                        key={dateString(day)}
                        role="gridcell"
                        data-part="calendar.slot"
                        data-slot={`${dayIndex}-${hour}`}
                        data-day={dateString(day)}
                        data-state={day.compare(today) === 0 ? "today" : undefined}
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
                          if (!active) setSlot({ day: dayIndex, hour });
                        }}
                        onDoubleClick={() => createAt(day, hour)}
                      />
                    );
                  })}
                </div>
              ))}
            </div>
            <div ref={columnsRef} data-part="calendar.columns" data-geom={columnsGeom}>
              {days.map((day, dayIndex) => {
                const placed = placeDay(instances, day, tz);
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
                      const top = (p.startMinute - hourStart * 60) / visibleMinutes;
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
                              left: pct(p.column / p.columns),
                              width: pct(1 / p.columns),
                            }}
                            part="calendar.event"
                            keys="time"
                            states={[
                              p.continuesBefore ? "continues-before" : null,
                              p.continuesAfter ? "continues-after" : null,
                              dragging ? "dragging" : null,
                            ].filter((x): x is string => x !== null)}
                            onDragStart={(event, mode) =>
                              startDrag(event, p.instance, dayIndex, mode)
                            }
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
              {todayIndex >= 0 && nowWall >= hourStart * 60 && nowWall < hourEnd * 60 && (
                <Positioned
                  geom={geomId(nodeKey, "now")}
                  part="calendar.now"
                  declarations={{
                    left: pct(todayIndex / days.length),
                    width: pct(1 / days.length),
                    top: pct((nowWall - hourStart * 60) / visibleMinutes),
                  }}
                />
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
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
  onCreate(): void;
}) {
  useGeometry(geom, { "grid-column": `${index + 1}`, "grid-row": `1 / span ${rows}` });
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
