/**
 * One occurrence in a calendar view: a button that opens the event editor,
 * named for screen readers with its title, time and calendar.
 *
 * Keyboard: Enter / Space open it; Delete removes it (asking the scope for
 * recurring events); Alt + arrows move it (time grids: Up / Down by 15
 * minutes, Left / Right by a day; day grids: by a day / a week); Alt + Shift
 * + Up / Down make it shorter / longer. The pointer drags it (and its
 * bottom edge resizes it in time grids) through `onDragStart`.
 *
 * M5b (WP164): the same readable card everywhere — title first, then the
 * time and the location (`event-card.title` / `.time` / `.location`), on a
 * light tint of the calendar colour with a bar in the colour (`colors.ts`);
 * the card's text is always the theme's `fg`.
 */
import type { KeyboardEvent, PointerEvent as ReactPointerEvent } from "react";
import type { Calendar, EventInstance } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { useGeometry } from "../../theme/ThemeProvider";
import { requestDelete, requestMove, movedTimes } from "./changes";
import { colorDeclarations, colorOf, useChangeContext } from "./common";
import { formatClock, formatPimTime } from "./fmt";
import { EVENT_DIALOG } from "./names";
import { usePim, usePimStore } from "./store";
import { sameTime } from "./time";

export type DragMode = "move" | "resize";

export interface EventItemProps {
  instance: EventInstance;
  tz: string;
  calendars: readonly Calendar[] | undefined;
  /** Geometry id and position declarations (time grids, spanning rows). */
  geom: string;
  position?: Record<string, string>;
  part: string;
  /** How Alt + arrows move it. */
  keys: "time" | "days";
  /** Extra states (`continues-before`, `dragging`, …). */
  states?: readonly string[];
  /** Pointer drag start (move or, from the bottom edge, resize). */
  onDragStart?(event: ReactPointerEvent<HTMLElement>, mode: DragMode): void;
  /** Swallow the click that ends a drag. */
  suppressClick?(): boolean;
}

/** Opens the event editor on an occurrence. */
export function useOpenInstance() {
  const ctx = useChangeContext();
  return (instance: EventInstance) => {
    const pim = ctx.pim.getState();
    pim.select({ eventId: instance.event_id, recurrenceId: instance.recurrence_id });
    pim.editEvent({
      mode: "edit",
      eventId: instance.event_id,
      recurrenceId: instance.recurrence_id,
      start: instance.start,
      end: instance.end,
    });
    ctx.store.getState().openWindow(EVENT_DIALOG);
  };
}

export function EventItem({
  instance,
  tz,
  calendars,
  geom,
  position = {},
  part,
  keys,
  states = [],
  onDragStart,
  suppressClick,
}: EventItemProps) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useChangeContext();
  const pimStore = usePimStore();
  const open = useOpenInstance();
  const selected = usePim(
    (s) =>
      s.selected !== null &&
      s.selected.eventId === instance.event_id &&
      ((s.selected.recurrenceId === null && instance.recurrence_id === null) ||
        (s.selected.recurrenceId !== null &&
          instance.recurrence_id !== null &&
          sameTime(s.selected.recurrenceId, instance.recurrence_id))),
  );
  const color = colorOf(calendars, instance.calendar_id, instance.color);
  useGeometry(geom, { ...position, ...colorDeclarations(color) });
  const calendar = calendars?.find((c) => c.id === instance.calendar_id);
  const time = instance.all_day
    ? t("pim-all-day")
    : t("pim-time-range", {
        start: formatClock(format, instance.start_utc, tz),
        end: formatClock(format, instance.end_utc, tz),
      });
  const title = instance.title || t("pim-event-untitled");
  const label = t("pim-event-label", {
    title,
    time,
    day: formatPimTime(format, instance.start, tz),
    calendar: calendar?.settings.name || "none",
    recurring: instance.recurring ? "yes" : "no",
  });

  const move = (days: number, minutes: number, resize: boolean) => {
    const { start, end } = movedTimes(instance, days, minutes, tz, resize);
    void requestMove(ctx, instance, start, end).catch((error: unknown) => {
      pimStore.getState().pushToast({
        kind: "problem",
        id: `move-${Date.now()}`,
        message: { id: "pim-move-failed", args: { error: String(error) } },
      });
    });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "Delete" || event.key === "Backspace") {
      if (instance.read_only) return;
      event.preventDefault();
      void requestDelete(ctx, instance);
      return;
    }
    if (!event.altKey || instance.read_only) return;
    const step = keys === "time" ? { up: [0, -15], down: [0, 15] } : { up: [-7, 0], down: [7, 0] };
    switch (event.key) {
      case "ArrowUp":
        event.preventDefault();
        if (event.shiftKey && keys === "time") move(0, -15, true);
        else move(step.up[0] as number, step.up[1] as number, false);
        break;
      case "ArrowDown":
        event.preventDefault();
        if (event.shiftKey && keys === "time") move(0, 15, true);
        else move(step.down[0] as number, step.down[1] as number, false);
        break;
      case "ArrowLeft":
        event.preventDefault();
        move(-1, 0, false);
        break;
      case "ArrowRight":
        event.preventDefault();
        move(1, 0, false);
        break;
    }
  };

  const allStates = [
    ...states,
    selected ? "selected" : null,
    instance.recurring ? "recurring" : null,
    instance.overridden ? "overridden" : null,
    instance.read_only ? "read-only" : null,
    instance.status === "tentative" ? "tentative" : null,
    instance.transparency === "free" ? "free" : null,
    instance.all_day ? "all-day" : null,
  ].filter(Boolean);

  return (
    <button
      type="button"
      data-part={part}
      data-geom={geom}
      data-event={instance.event_id}
      data-state={allStates.join(" ") || undefined}
      aria-label={label}
      onClick={() => {
        if (suppressClick?.()) return;
        open(instance);
      }}
      onFocus={() =>
        pimStore
          .getState()
          .select({ eventId: instance.event_id, recurrenceId: instance.recurrence_id })
      }
      onKeyDown={onKeyDown}
      onPointerDown={
        onDragStart && !instance.read_only ? (event) => onDragStart(event, "move") : undefined
      }
    >
      <span data-part="event-card.title">{title}</span>
      <span data-part="event-card.time">{time}</span>
      {instance.location && <span data-part="event-card.location">{instance.location}</span>}
      {instance.recurring && (
        <span data-part="event-card.badge" data-badge="recurring">
          {t("pim-badge-recurring")}
        </span>
      )}
      {onDragStart && keys === "time" && !instance.read_only && (
        <span
          data-part={`${part}.resize`}
          aria-hidden="true"
          onPointerDown={(event) => {
            event.stopPropagation();
            onDragStart(event, "resize");
          }}
        />
      )}
    </button>
  );
}
