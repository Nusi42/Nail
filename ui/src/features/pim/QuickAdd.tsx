/**
 * Quick create from text (S2, ADR 0246): a field in the calendar toolbar
 * that reads a sentence ("Lunch with Bob tomorrow 12:30 at Corner café")
 * through the core's `ParseEventText` and previews what it understood as
 * chips while the user types. Enter adds the event to the default calendar
 * and shows its day, Tab opens the event editor prefilled, Esc empties the
 * field. Nothing is saved before Enter.
 *
 * Parts: `calendar.quick-add` (the form), `form.input`
 * (`data-field="quick-add"`), `calendar.quick-add.chips` /
 * `calendar.quick-add.chip` (`data-kind`: title, date, time, recurrence,
 * location, calendar; resolved values, not the words as typed),
 * `calendar.quick-add.status` (polite).
 *
 * R8 (ADR 0251): the same field is the window "New event from text" of the
 * command palette (`EventFromTextDialog`): it stays open for the next
 * event after Enter, and Tab closes it when the editor opens.
 */
import { useQuery } from "@tanstack/react-query";
import { useId, useState, type KeyboardEvent } from "react";
import type { CalendarDate } from "@internationalized/date";
import type { EventFields, ParsedEventText, PimTime } from "../../api/generated";
import { DEFAULT_EVENT_MINUTES, MAX_EVENT_TEXT } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { TYPING_PAUSE_MS, useDebounced } from "../../state/debounce";
import { Icon } from "../../widgets/Icon";
import { problemOf, useChangeContext } from "./common";
import { emptyEventFields } from "./defaults";
import { formatClock, formatDay, formatPimTime } from "./fmt";
import { openNewEvent, defaultEventCalendar } from "./newEvent";
import { parseEventText } from "./prefs";
import { choiceOf } from "./recurrence";
import { useRecurrenceText } from "./RecurrenceEditor";
import { saveEvent, useCalendars } from "./queries";
import {
  dateString,
  localZone,
  makePimTime,
  pimTimeIn,
  shiftPimTime,
  toCalendarDate,
  todayIn,
} from "./time";
import type { DialogProps } from "../../layout/dialogRegistry";
import {
  DEFAULT_NEW_EVENT_TIME,
  quickChips,
  type QuickChipKind,
  type QuickChipWords,
} from "./viewsModel";

/** The start and end of a quick-created event: the parsed ones, else the day at the default time. */
export function quickTimes(
  parsed: ParsedEventText,
  day: CalendarDate,
  tz: string,
): { start: PimTime; end: PimTime } {
  if (parsed.start && parsed.end) return { start: parsed.start, end: parsed.end };
  const start = makePimTime(day, DEFAULT_NEW_EVENT_TIME, tz);
  return { start, end: shiftPimTime(start, 0, DEFAULT_EVENT_MINUTES) };
}

/** The event fields of a parsed text (the whole text as the title when nothing else is left). */
export function quickFields(
  text: string,
  parsed: ParsedEventText,
  day: CalendarDate,
  tz: string,
): EventFields {
  const { start, end } = quickTimes(parsed, day, tz);
  return {
    ...emptyEventFields(start, end),
    title: parsed.title.trim() || text.trim(),
    location: parsed.location,
  };
}

/** The label of each kind of chip in the preview (the title has none). */
export const QUICK_CHIP_LABELS: Readonly<Record<Exclude<QuickChipKind, "title">, string>> = {
  date: "common-date",
  time: "common-time",
  recurrence: "common-repeats",
  location: "pim-views-quick-add-kind-location",
  calendar: "common-calendar",
};

/** The preview's words in the UI language (`quickChips`). */
function useQuickChipWords(tz: string): QuickChipWords {
  const t = useT();
  const format = useDateFormat();
  const recurrenceText = useRecurrenceText();
  const day = (d: CalendarDate) =>
    formatDay(format, d, { weekday: "short", month: "short", day: "numeric" });
  const clock = (time: PimTime) => formatClock(format, pimTimeIn(time, tz).toDate().getTime(), tz);
  return {
    day,
    days: (first, last) => t("common-time-range-all-day", { start: day(first), end: day(last) }),
    range: (start, end) => t("common-time-range", { start: clock(start), end: clock(end) }),
    allDay: t("common-all-day"),
    recurrence: (recurrence, start) => recurrenceText(choiceOf(recurrence, start), start),
  };
}

export interface QuickAddProps {
  /** The day of a text without a date. */
  day: CalendarDate;
  tz: string;
  /** After Tab handed the text to the editor (the window closes). */
  onEditor?: () => void;
  /** Takes the focus when it shows (a window of its own). */
  focusOnShow?: boolean;
}

export function QuickAdd({ day, tz, onEditor, focusOnShow = false }: QuickAddProps) {
  const t = useT();
  const format = useDateFormat();
  const locale = useLocale();
  const ctx = useChangeContext();
  const ids = { input: useId(), chips: useId() };
  const calendars = useCalendars();
  const [text, setText] = useState("");
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const settled = useDebounced(text.trim(), TYPING_PAUSE_MS);
  const queryKey = ["pim", "parse-event-text", settled, tz, locale] as const;
  const preview = useQuery({
    queryKey,
    queryFn: () => parseEventText(ctx.client, settled, tz, locale),
    enabled: settled !== "",
  });
  const target = defaultEventCalendar(calendars.data ?? []);
  const words = useQuickChipWords(tz);
  const chips =
    settled !== "" && preview.data
      ? quickChips(
          preview.data,
          quickTimes(preview.data, day, tz),
          target?.settings.name ?? null,
          words,
        )
      : [];

  /** The reading of the current text (the preview's when it is current). */
  const parse = async (current: string) =>
    ctx.queryClient.query({
      queryKey: ["pim", "parse-event-text", current, tz, locale] as const,
      queryFn: () => parseEventText(ctx.client, current, tz, locale),
    });

  const add = async () => {
    const current = text.trim();
    if (current === "" || !target || busy) return;
    setBusy(true);
    try {
      const parsed = await parse(current);
      const fields = quickFields(current, parsed, day, tz);
      await saveEvent(ctx, { mode: "create", calendarId: target.id }, fields, parsed.recurrence);
      const shown = toCalendarDate(pimTimeIn(fields.start, tz));
      ctx.pim.getState().setAnchor(dateString(shown));
      setText("");
      setStatus(
        t("pim-views-quick-add-added", {
          title: fields.title,
          when: formatPimTime(format, fields.start, tz),
        }),
      );
    } catch (error) {
      const problem = problemOf(error);
      setStatus(t(problem.id, problem.args));
    } finally {
      setBusy(false);
    }
  };

  const openEditor = async () => {
    const current = text.trim();
    const parsed = await parse(current);
    const fields = quickFields(current, parsed, day, tz);
    setText("");
    onEditor?.();
    openNewEvent(ctx, day, null, tz, target?.id ?? null, fields, parsed.recurrence);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      void add();
    } else if (event.key === "Tab" && !event.shiftKey && text.trim() !== "") {
      event.preventDefault();
      void openEditor();
    } else if (event.key === "Escape" && text !== "") {
      event.preventDefault();
      event.stopPropagation();
      setText("");
      setStatus("");
    }
  };

  if (!target) return null;
  return (
    <div data-part="calendar.quick-add">
      <Icon name="plus" part="calendar.quick-add.icon" />
      <input
        id={ids.input}
        data-part="form.input"
        data-field="quick-add"
        type="text"
        value={text}
        maxLength={MAX_EVENT_TEXT}
        aria-label={t("pim-views-quick-add-label")}
        aria-describedby={chips.length > 0 ? ids.chips : undefined}
        aria-busy={busy}
        placeholder={t("pim-views-quick-add-placeholder")}
        onChange={(e) => {
          setText(e.target.value);
          setStatus("");
        }}
        onKeyDown={onKeyDown}
        // eslint-disable-next-line jsx-a11y/no-autofocus -- only in the window "New event from text", which opens on request for this field (dialog pattern).
        autoFocus={focusOnShow}
      />
      {chips.length > 0 && text.trim() !== "" && (
        <ul
          id={ids.chips}
          data-part="calendar.quick-add.chips"
          aria-label={t("pim-views-quick-add-preview")}
        >
          {chips.map((chip, i) => (
            <li
              key={`${chip.kind}-${String(i)}`}
              data-part="calendar.quick-add.chip"
              data-kind={chip.kind}
            >
              {chip.kind !== "title" && (
                <span data-part="calendar.quick-add.chip.kind">
                  {t(QUICK_CHIP_LABELS[chip.kind])}
                </span>
              )}
              <span data-part="calendar.quick-add.chip.text">{chip.text}</span>
            </li>
          ))}
          <li data-part="calendar.quick-add.hint">{t("pim-views-quick-add-hint")}</li>
        </ul>
      )}
      <p data-part="calendar.quick-add.status" aria-live="polite">
        {status}
      </p>
    </div>
  );
}

/** The window "New event from text" (R8, ADR 0251; the palette's `calendar.new-event-from-text`). */
export function EventFromTextDialog({ close }: DialogProps) {
  const t = useT();
  const tz = localZone();
  const calendars = useCalendars();
  const writable = defaultEventCalendar(calendars.data ?? []) !== null;
  return (
    <div data-part="pim.from-text">
      <p data-part="pim.editor.hint">
        {calendars.isPending
          ? t("common-loading")
          : writable
            ? t("pim-act-from-text-intro")
            : t("pim-act-from-text-no-calendar")}
      </p>
      <QuickAdd day={todayIn(tz)} tz={tz} onEditor={close} focusOnShow />
    </div>
  );
}
