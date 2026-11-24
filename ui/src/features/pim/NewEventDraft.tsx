/**
 * The inline title of a new event drawn on the time grid (R7 M5, ADR
 * 0246): a popover beside the drawn range with a title field, the times
 * and the calendar the event goes to. Enter (or Save) saves it with only
 * the title; "More options" opens the full editor with the times and what
 * was typed; Esc or a press outside drops it.
 */
import { useState } from "react";
import type { Calendar } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { Popover, type AnchorRect } from "../../widgets/Popover";
import { problemOf, useChangeContext } from "./common";
import { formatDay, formatMinutes } from "./fmt";
import { createEventIn, defaultEventCalendar, openNewEventIn } from "./newEvent";
import { usePimStore } from "./store";
import type { CalendarDate, MinuteRange } from "./time";

/** The length of a range as Fluent arguments (`pim-grid-length`). */
export function lengthArgs(range: MinuteRange): { hours: number; minutes: number } {
  const total = Math.max(0, range.end - range.start);
  return { hours: Math.floor(total / 60), minutes: total % 60 };
}

/** `10:00 AM – 11:30 AM` for a range of minutes after midnight. */
export function useRangeText(): (range: MinuteRange) => string {
  const t = useT();
  const format = useDateFormat();
  return (range) =>
    t("common-time-range", {
      start: formatMinutes(format, range.start),
      end: formatMinutes(format, range.end),
    });
}

export interface NewEventDraftProps {
  /** Where the drawn range is on screen (the popover's anchor). */
  anchor: AnchorRect;
  day: CalendarDate;
  range: MinuteRange;
  tz: string;
  calendars: readonly Calendar[] | undefined;
  onClose: () => void;
}

export function NewEventDraft({ anchor, day, range, tz, calendars, onClose }: NewEventDraftProps) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useChangeContext();
  const pim = usePimStore();
  const rangeText = useRangeText();
  const [title, setTitle] = useState("");
  const [saving, setSaving] = useState(false);
  const calendar = defaultEventCalendar(calendars ?? []);

  const save = () => {
    if (calendar === null || title.trim() === "" || saving) return;
    setSaving(true);
    createEventIn(ctx, calendar, day, range, tz, title.trim()).then(onClose, (error: unknown) => {
      setSaving(false);
      pim.getState().pushToast({
        kind: "problem",
        id: `create-${String(Date.now())}`,
        message: problemOf(error),
      });
    });
  };

  return (
    <Popover anchor={anchor} label={t("pim-action-new-event")} onClose={onClose}>
      <form
        data-part="calendar.new-event"
        onSubmit={(event) => {
          event.preventDefault();
          save();
        }}
      >
        <input
          data-part="form.input"
          data-field="new-event-title"
          data-autofocus
          type="text"
          value={title}
          disabled={saving}
          aria-label={t("pim-event-title")}
          placeholder={t("pim-event-title-placeholder")}
          onChange={(event) => {
            setTitle(event.target.value);
          }}
        />
        <span data-part="calendar.new-event.when">
          {t("pim-grid-new-event-when", {
            day: formatDay(format, day, { weekday: "short", month: "short", day: "numeric" }),
            time: rangeText(range),
          })}
        </span>
        {calendar && <span data-part="calendar.new-event.calendar">{calendar.settings.name}</span>}
        <div data-part="calendar.new-event.actions">
          <button
            type="button"
            data-part="form.button"
            onClick={() => {
              onClose();
              openNewEventIn(ctx, day, range, tz, calendar, title.trim());
            }}
          >
            {t("pim-more-options")}
          </button>
          <button
            type="submit"
            data-part="form.button"
            data-variant="primary"
            disabled={calendar === null || title.trim() === "" || saving}
          >
            {t("common-save")}
          </button>
        </div>
      </form>
    </Popover>
  );
}
