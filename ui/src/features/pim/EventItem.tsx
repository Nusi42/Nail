/**
 * One occurrence in a calendar view: a button that opens the quick look
 * (R7 M4: a click, Enter or Space; a double-click opens the full editor),
 * named for screen readers with its title, time and calendar.
 *
 * Keyboard: Enter / Space open the quick look; Delete removes it (asking the scope for
 * recurring events); Alt + arrows move it (time grids: Up / Down by 15
 * minutes, Left / Right by a day; day grids: by a day / a week); Alt + Shift
 * + Up / Down make it shorter / longer. The pointer drags it (and its
 * bottom edge resizes it in time grids) through `onDragStart`.
 *
 * M5b (WP164): the same readable card everywhere — title first, then the
 * time and the location (`event-card.title` / `.time` / `.location`), on a
 * light tint of the calendar colour with a bar in the colour (`colors.ts`);
 * the card's text is always the theme's `fg`.
 *
 * R7 (ADR 0246): a short card (`data-size="short"`, M3) shows one line,
 * the title then the start time; small icons (`event-card.icon`, S4) mark
 * repeats, people, reminders and read-only events and hide when the card
 * is narrow; free time is hatched (state `free`); an unanswered invitation
 * (`my_status` needs-action, S1) is provisional: dashed outline, no fill,
 * a question mark; a declined one is struck through.
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
import { nonEmpty } from "../../text/empty";
import { anchorOf } from "../../widgets/Popover";
import { Icon } from "../../widgets/Icon";

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
  /**
   * The visible time: the range (default) or only the start (the month
   * view's chips, where the title needs the width; the label keeps both).
   */
  timeStyle?: "range" | "start";
  /** A short one-line card (R7 M3, `data-size="short"`). */
  short?: boolean;
  /** Extra states (`continues-before`, `dragging`, …). */
  states?: readonly string[];
  /** Pointer drag start (move or, from the bottom edge, resize). */
  onDragStart?: (event: ReactPointerEvent<HTMLElement>, mode: DragMode) => void;
  /** Swallow the click that ends a drag. */
  suppressClick?: () => boolean;
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
  timeStyle = "range",
  short = false,
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
    ? t("common-all-day")
    : t("common-time-range", {
        start: formatClock(format, instance.start_utc, tz),
        end: formatClock(format, instance.end_utc, tz),
      });
  const shownTime =
    instance.all_day || (timeStyle === "range" && !short)
      ? time
      : formatClock(format, instance.start_utc, tz);
  const title = instance.title || t("common-no-title");
  const label = t("pim-grid-event-label", {
    label: t("pim-event-label", {
      title,
      time,
      day: formatPimTime(format, instance.start, tz),
      calendar: nonEmpty(calendar?.settings.name) ?? "none",
      recurring: instance.recurring ? "yes" : "no",
    }),
    answer: instance.my_status ?? "none",
  });

  const move = (days: number, minutes: number, resize: boolean) => {
    const { start, end } = movedTimes(instance, days, minutes, tz, resize);
    void requestMove(ctx, instance, start, end).catch((error: unknown) => {
      pimStore.getState().pushToast({
        kind: "problem",
        id: `move-${String(Date.now())}`,
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
    const step: { up: [number, number]; down: [number, number] } =
      keys === "time" ? { up: [0, -15], down: [0, 15] } : { up: [-7, 0], down: [7, 0] };
    switch (event.key) {
      case "ArrowUp":
        event.preventDefault();
        if (event.shiftKey && keys === "time") move(0, -15, true);
        else move(step.up[0], step.up[1], false);
        break;
      case "ArrowDown":
        event.preventDefault();
        if (event.shiftKey && keys === "time") move(0, 15, true);
        else move(step.down[0], step.down[1], false);
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
    instance.my_status === "needs-action" ? "needs-action" : null,
    instance.my_status === "declined" ? "declined" : null,
  ].filter(Boolean);
  const icons = [
    instance.my_status === "needs-action" ? "circle-question-mark" : null,
    instance.recurring ? "refresh-cw" : null,
    instance.has_attendees ? "users" : null,
    instance.has_alarms ? "bell" : null,
    instance.read_only ? "lock" : null,
  ].filter((name): name is string => name !== null);

  return (
    <button
      type="button"
      data-part={part}
      data-geom={geom}
      data-event={instance.event_id}
      data-state={allStates.join(" ") || undefined}
      data-size={short ? "short" : undefined}
      aria-label={label}
      onClick={(event) => {
        if (suppressClick?.()) return;
        pimStore.getState().setQuickLook({ instance, anchor: anchorOf(event.currentTarget) });
      }}
      onDoubleClick={() => {
        pimStore.getState().setQuickLook(null);
        open(instance);
      }}
      onFocus={() => {
        pimStore
          .getState()
          .select({ eventId: instance.event_id, recurrenceId: instance.recurrence_id });
      }}
      onKeyDown={onKeyDown}
      onPointerDown={
        onDragStart && !instance.read_only
          ? (event) => {
              onDragStart(event, "move");
            }
          : undefined
      }
    >
      <span data-part="event-card.title">{title}</span>
      <span data-part="event-card.time">{shownTime}</span>
      {instance.location && !short && (
        <span data-part="event-card.location">{instance.location}</span>
      )}
      {icons.length > 0 && (
        <span data-part="event-card.icons" aria-hidden="true">
          {icons.map((name) => (
            <Icon key={name} name={name} part="event-card.icon" />
          ))}
        </span>
      )}
      {instance.recurring && (
        <span data-part="event-card.badge" data-badge="recurring">
          {t("common-repeats")}
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
