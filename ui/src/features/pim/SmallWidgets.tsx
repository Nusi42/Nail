/**
 * The small calendar widgets for any window (design: "Mini-month and
 * today's agenda are available as widgets for the mail view"):
 *
 * - `mini-month`: a month to navigate the calendar views with (days with
 *   events are marked); choosing a day shows it in every calendar widget.
 * - `agenda`: today's (option `days`: the next days') occurrences.
 *
 * Both follow the shared day of the calendar views; options `time-zone`
 * and `calendars` as for `calendar`.
 */
import { startOfMonth } from "@internationalized/date";
import type { CalendarId } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { optionList, optionNumber, type WidgetProps } from "../../layout/widgetRegistry";
import { useNow } from "../../state/clock";
import { Icon } from "../../widgets/Icon";
import { useAnchor } from "./Calendar";
import { AgendaList } from "./CalendarViews";
import { Problem, problemOf, useFollowingDay } from "./common";
import { EmptyState } from "./EmptyState";
import { dayDate } from "./fmt";
import { MonthGrid } from "./MonthGrid";
import { useCalendarPreferences, weekStartOf } from "./prefs";
import { useCalendars, useInstances } from "./queries";
import { usePim, usePimStore } from "./store";
import {
  DEFAULT_VIEW_OPTIONS,
  busyDays,
  dateString,
  daysOf,
  rangeInstants,
  todayIn,
  viewRange,
  visibleInstances,
  type ViewKind,
} from "./time";
import { useCalendarZones } from "./viewHooks";
import { dayDots } from "./viewsModel";

/** Views whose days the mini-month marks as a band (a month or year shows anyway). */
const BAND_VIEWS: readonly ViewKind[] = [
  "day",
  "multi-day",
  "week",
  "work-week",
  "agenda",
  "free-time",
];

export function MiniMonth({ node }: WidgetProps) {
  const calendarIds = optionList(node, "calendars", []);
  return (
    <MiniMonthView
      tz={useCalendarZones(node).zone}
      calendarIds={calendarIds.length > 0 ? calendarIds : null}
    />
  );
}

/**
 * The mini-month (S5, ADR 0246): the range the calendar views show as a
 * band, a dot per calendar colour on days with events, today's mark on any
 * month, ISO week numbers when the preference is on. The calendar's drawer
 * shows it too.
 */
export function MiniMonthView({
  tz,
  calendarIds,
}: {
  tz: string;
  calendarIds: readonly CalendarId[] | null;
}) {
  const t = useT();
  const format = useDateFormat();
  const locale = useLocale();
  const pim = usePimStore();
  const prefs = useCalendarPreferences();
  const shown = usePim((s) => s.shown);
  const anchor = useAnchor(tz);
  const today = todayIn(tz, useNow());
  const [focused, setFocused] = useFollowingDay(anchor);
  const month = startOfMonth(focused);
  const weekStart = weekStartOf(prefs);
  const range = viewRange("month", month, locale, { ...DEFAULT_VIEW_OPTIONS, weekStart });
  const instances = useInstances(rangeInstants(range, tz), calendarIds, tz);
  const calendars = useCalendars();
  const list = visibleInstances(instances.data ?? [], prefs.show_declined);
  const busy = busyDays(list, tz);
  const dots = dayDots(list, calendars.data, daysOf(range), tz);
  const band =
    shown && BAND_VIEWS.includes(shown.view)
      ? viewRange(shown.view, anchor, locale, {
          ...DEFAULT_VIEW_OPTIONS,
          days: shown.days,
          weekStart,
        })
      : null;
  const monthName = format.dateTime(dayDate(month), {
    month: "long",
    year: "numeric",
    timeZone: "UTC",
  });
  return (
    <section data-part="mini-month" aria-label={t("pim-mini-month-label", { month: monthName })}>
      <div data-part="mini-month.header">
        <button
          type="button"
          data-part="form.button"
          data-variant="icon ghost"
          data-field="mini-month-previous"
          aria-label={t("pim-month-previous")}
          title={t("pim-month-previous")}
          onClick={() => {
            setFocused((f) => f.add({ months: -1 }));
          }}
        >
          <Icon name="chevron-left" part="form.button.icon" />
        </button>
        <h2 data-part="mini-month.title" aria-live="polite">
          {monthName}
        </h2>
        <button
          type="button"
          data-part="form.button"
          data-variant="icon ghost"
          data-field="mini-month-next"
          aria-label={t("pim-month-next")}
          title={t("pim-month-next")}
          onClick={() => {
            setFocused((f) => f.add({ months: 1 }));
          }}
        >
          <Icon name="chevron-right" part="form.button.icon" />
        </button>
      </div>
      <MonthGrid
        part="mini-month.grid"
        month={month}
        focused={focused}
        selected={anchor}
        today={today}
        busy={busy}
        dots={dots}
        weekStart={weekStart}
        band={band}
        weekNumbers={prefs.show_week_numbers}
        label={monthName}
        narrow
        onFocusDay={setFocused}
        onSelect={(day) => {
          pim.getState().setAnchor(dateString(day));
        }}
      />
    </section>
  );
}

export function Agenda({ node, nodeKey }: WidgetProps) {
  const t = useT();
  const tz = useCalendarZones(node).zone;
  const prefs = useCalendarPreferences();
  const today = todayIn(tz, useNow());
  const days = Math.min(31, Math.max(1, Math.trunc(optionNumber(node, "days", 1))));
  const range = { start: today, count: days };
  const calendarIds = optionList(node, "calendars", []);
  const instances = useInstances(
    rangeInstants(range, tz),
    calendarIds.length > 0 ? calendarIds : null,
    tz,
  );
  const calendars = useCalendars();
  return (
    <div data-part="agenda" data-key-context="calendar">
      <h2 data-part="agenda.title">
        {days === 1 ? t("pim-agenda-today") : t("pim-agenda-next", { days })}
      </h2>
      {instances.isError && <Problem message={problemOf(instances.error)} />}
      <AgendaList
        nodeKey={nodeKey}
        range={range}
        instances={visibleInstances(instances.data ?? [], prefs.show_declined)}
        calendars={calendars.data}
        tz={tz}
        part="agenda.list"
        empty={
          <EmptyState
            view="agenda-widget"
            icon="calendar-check"
            text={days === 1 ? t("pim-views-empty-today-agenda") : t("pim-agenda-empty")}
          />
        }
        label={days === 1 ? t("pim-agenda-today") : t("pim-agenda-next", { days })}
      />
    </div>
  );
}
