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
import { useDateFormat } from "../../i18n/format";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { optionList, optionNumber, type WidgetProps } from "../../layout/widgetRegistry";
import { useNow } from "../../state/clock";
import { Icon } from "../../widgets/Icon";
import { useAnchor } from "./Calendar";
import { AgendaList } from "./CalendarViews";
import { Problem, problemOf, useFollowingDay, useViewZone } from "./common";
import { dayDate } from "./fmt";
import { MonthGrid } from "./MonthGrid";
import { useCalendars, useInstances } from "./queries";
import { usePimStore } from "./store";
import { busyDays, dateString, rangeInstants, todayIn, viewRange } from "./time";

export function MiniMonth({ node }: WidgetProps) {
  const t = useT();
  const format = useDateFormat();
  const locale = useLocale();
  const pim = usePimStore();
  const tz = useViewZone(node);
  const anchor = useAnchor(tz);
  const today = todayIn(tz, useNow());
  const [focused, setFocused] = useFollowingDay(anchor);
  const month = startOfMonth(focused);
  const range = viewRange("month", month, locale);
  const calendarIds = optionList(node, "calendars", []);
  const instances = useInstances(
    rangeInstants(range, tz),
    calendarIds.length > 0 ? calendarIds : null,
    tz,
  );
  const busy = busyDays(instances.data ?? [], tz);
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
          onClick={() => setFocused((f) => f.add({ months: -1 }))}
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
          onClick={() => setFocused((f) => f.add({ months: 1 }))}
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
        label={monthName}
        narrow
        onFocusDay={setFocused}
        onSelect={(day) => pim.getState().setAnchor(dateString(day))}
      />
    </section>
  );
}

export function Agenda({ node, nodeKey }: WidgetProps) {
  const t = useT();
  const tz = useViewZone(node);
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
        instances={instances.data ?? []}
        calendars={calendars.data}
        tz={tz}
        part="agenda.list"
        label={days === 1 ? t("pim-agenda-today") : t("pim-agenda-next", { days })}
      />
    </div>
  );
}
