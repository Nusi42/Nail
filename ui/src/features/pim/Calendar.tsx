/**
 * The `calendar` widget: one calendar view (option `view`: day, multi-day,
 * week, work-week, month, year, agenda, free-time) with an optional header
 * (previous / today / next, the range shown, the view switcher, "New
 * event"). Every calendar, mini-month and agenda widget shows the same day
 * (`PimState.anchor`); `calendar.view` switches the views of all calendar
 * widgets.
 *
 * Options: `view`, `views` (the switcher's views), `days` (multi-day,
 * agenda, free-time), `calendars` (ids; default all visible), `time-zone`,
 * `secondary-zone`, `hour-start`, `hour-end`, `scroll-to`, `max-events`
 * (month), `duration` (free-time minutes), `work-start` / `work-end` (the
 * free-time timeline's hours), `header`, `view-switch` (`segmented`, the
 * default, or `menu`).
 *
 * M5b (WP164): the header is one toolbar line that never wraps — Today,
 * previous / next, the range shown (cut with an ellipsis when the pane is
 * narrow), the segmented view switch and "New event". When the segmented
 * switch does not fit, the view menu replaces it (`switchFit.ts`).
 * Keys: the widget is the `calendar` key context.
 */
import type { CalendarDate } from "@internationalized/date";
import { useEffect, useId, useRef } from "react";
import type { ViewKind } from "./time";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { useLocale } from "../../i18n/I18nProvider";
import {
  optionBool,
  optionList,
  optionNumber,
  optionString,
  type WidgetProps,
} from "../../layout/widgetRegistry";
import { useNow } from "../../state/clock";
import { Icon } from "../../widgets/Icon";
import { AgendaList, FreeTimeView, MonthView, YearView } from "./CalendarViews";
import { Problem, problemOf, useChangeContext, useViewZone } from "./common";
import { dayDate, formatDay } from "./fmt";
import { openNewEvent } from "./newEvent";
import { useCalendars, useInstances } from "./queries";
import { usePim, usePimStore } from "./store";
import { useNarrowViewSwitch } from "./switchFit";
import { TimeGrid } from "./TimeGrid";
import {
  DEFAULT_VIEW_OPTIONS,
  VIEW_KINDS,
  daysOf,
  isKnownZone,
  isViewKind,
  parseDay,
  rangeInstants,
  stepAnchor,
  todayIn,
  viewRange,
  dateString,
  type ViewOptions,
  type ViewRange,
} from "./time";
import type { DateFormatter } from "../../i18n/format";
import type { MessageArgs } from "../../i18n/localizer";

const TIME_GRID_VIEWS: readonly ViewKind[] = ["day", "multi-day", "week", "work-week"];

/** The anchor day of the views (today when none is set). */
export function useAnchor(tz: string): CalendarDate {
  const anchor = usePim((s) => s.anchor);
  const today = todayIn(tz, useNow());
  return (anchor ? parseDay(anchor) : null) ?? today;
}

/** The text naming the days a view shows. */
export function rangeTitle(
  view: ViewKind,
  range: ViewRange,
  anchor: CalendarDate,
  format: DateFormatter,
  t: (id: string, args?: MessageArgs) => string,
): string {
  switch (view) {
    case "day":
      return formatDay(format, anchor, { dateStyle: "full" });
    case "month":
      return format.dateTime(dayDate(anchor), { month: "long", year: "numeric", timeZone: "UTC" });
    case "year":
      return format.dateTime(dayDate(anchor), { year: "numeric", timeZone: "UTC" });
    default: {
      const last = range.start.add({ days: range.count - 1 });
      return t("pim-range", {
        start: formatDay(format, range.start, { day: "numeric", month: "short" }),
        end: formatDay(format, last, { dateStyle: "medium" }),
      });
    }
  }
}

function viewOptions(node: WidgetProps["node"], view: ViewKind): ViewOptions {
  const fallbackDays =
    view === "agenda" ? 14 : view === "free-time" ? 5 : DEFAULT_VIEW_OPTIONS.days;
  return {
    ...DEFAULT_VIEW_OPTIONS,
    days: Math.min(62, Math.max(1, Math.trunc(optionNumber(node, "days", fallbackDays)))),
  };
}

function clampHour(value: number, fallback: number): number {
  return Number.isFinite(value) ? Math.min(24, Math.max(0, Math.trunc(value))) : fallback;
}

/**
 * The view switch: a segmented row of toggle buttons (one pressed), or with
 * `view-switch="menu"` a drop-down for narrow panes.
 */
function ViewSwitch({
  views,
  view,
  menu,
  onChange,
}: {
  views: readonly ViewKind[];
  view: ViewKind;
  menu: boolean;
  onChange: (view: ViewKind) => void;
}) {
  const t = useT();
  const id = useId();
  if (menu) {
    return (
      <div data-part="calendar.view-menu">
        <label data-part="form.label" htmlFor={id}>
          {t("pim-views-label")}
        </label>
        <select
          id={id}
          data-part="form.select"
          data-field="calendar-view"
          value={view}
          onChange={(e) => {
            if (isViewKind(e.target.value)) onChange(e.target.value);
          }}
        >
          {views.map((kind) => (
            <option key={kind} value={kind}>
              {t(`pim-view-${kind}`)}
            </option>
          ))}
        </select>
      </div>
    );
  }
  return (
    <div
      data-part="segmented"
      data-field="calendar-view"
      role="group"
      aria-label={t("pim-views-label")}
    >
      {views.map((kind) => (
        <button
          key={kind}
          type="button"
          data-part="segmented.option"
          data-value={kind}
          data-view={kind}
          aria-pressed={kind === view}
          onClick={() => {
            onChange(kind);
          }}
        >
          {t(`pim-view-${kind}`)}
        </button>
      ))}
    </div>
  );
}

export function CalendarWidget({ node, nodeKey }: WidgetProps) {
  const t = useT();
  const format = useDateFormat();
  const locale = useLocale();
  const ctx = useChangeContext();
  const pim = usePimStore();
  const tz = useViewZone(node);
  const secondaryOption = optionString(node, "secondary-zone", "");
  const secondaryTz = secondaryOption && isKnownZone(secondaryOption) ? secondaryOption : null;
  const own = optionString(node, "view", "week");
  const override = usePim((s) => s.view);
  const view: ViewKind = override ?? (isViewKind(own) ? own : "week");
  const views = optionList(node, "views", VIEW_KINDS).filter(isViewKind);
  const calendarIds = optionList(node, "calendars", []);
  const header = optionBool(node, "header", true);
  const switchStyle = optionString(node, "view-switch", "segmented");
  const hourStart = clampHour(optionNumber(node, "hour-start", 0), 0);
  const hourEnd = Math.max(hourStart + 1, clampHour(optionNumber(node, "hour-end", 24), 24));
  const anchor = useAnchor(tz);
  const options = viewOptions(node, view);
  const range = viewRange(view, anchor, locale, options);
  const instances = useInstances(
    rangeInstants(range, tz),
    calendarIds.length > 0 ? calendarIds : null,
    tz,
  );
  const calendars = useCalendars();
  const today = todayIn(tz, useNow());
  const title = rangeTitle(view, range, anchor, format, t);
  const toolbarRef = useRef<HTMLDivElement>(null);
  // Narrow panes get the view menu instead of a clipped segmented switch.
  const narrowSwitch = useNarrowViewSwitch(
    toolbarRef,
    header && switchStyle !== "menu" && views.length > 1,
    `${view} ${title}`,
  );

  useEffect(() => {
    pim.getState().setShown({ view, days: options.days });
  }, [pim, view, options.days]);

  const goTo = (day: CalendarDate) => {
    pim.getState().setAnchor(dateString(day));
  };
  const showDay = (day: CalendarDate) => {
    goTo(day);
    pim.getState().setView("day");
  };
  const list = instances.data ?? [];

  let body: React.ReactNode;
  if ((TIME_GRID_VIEWS as readonly string[]).includes(view)) {
    body = (
      <TimeGrid
        nodeKey={nodeKey}
        days={daysOf(range)}
        instances={list}
        calendars={calendars.data}
        tz={tz}
        secondaryTz={secondaryTz}
        hourStart={hourStart}
        hourEnd={hourEnd}
        scrollTo={clampHour(optionNumber(node, "scroll-to", 8), 8)}
        onDay={showDay}
      />
    );
  } else if (view === "month") {
    body = (
      <MonthView
        nodeKey={nodeKey}
        range={range}
        anchor={anchor}
        instances={list}
        calendars={calendars.data}
        tz={tz}
        onDay={showDay}
        maxPerDay={Math.max(1, Math.trunc(optionNumber(node, "max-events", 4)))}
      />
    );
  } else if (view === "year") {
    body = <YearView anchor={anchor} instances={list} tz={tz} onDay={showDay} />;
  } else if (view === "agenda") {
    body = (
      <AgendaList
        nodeKey={nodeKey}
        range={range}
        instances={list}
        calendars={calendars.data}
        tz={tz}
        label={t("pim-view-agenda")}
      />
    );
  } else {
    const workStart = clampHour(optionNumber(node, "work-start", 8), 8);
    body = (
      <FreeTimeView
        nodeKey={nodeKey}
        range={range}
        instances={list}
        tz={tz}
        minutes={Math.max(5, Math.trunc(optionNumber(node, "duration", 30)))}
        dayStartHour={workStart}
        dayEndHour={Math.max(workStart + 1, clampHour(optionNumber(node, "work-end", 18), 18))}
      />
    );
  }

  return (
    <section
      data-part="calendar"
      data-view={view}
      data-key-context="calendar"
      aria-label={t("pim-calendar-label", { view: t(`pim-view-${view}`), range: title })}
      aria-busy={instances.isFetching}
    >
      {header && (
        <div
          ref={toolbarRef}
          data-part="calendar.toolbar"
          data-switch={switchStyle === "menu" || narrowSwitch ? "menu" : "segmented"}
        >
          <div data-part="calendar.nav" role="group" aria-label={t("pim-nav-label")}>
            <button
              type="button"
              data-part="form.button"
              data-field="calendar-today"
              aria-current={anchor.compare(today) === 0 ? "date" : undefined}
              onClick={() => {
                pim.getState().setAnchor(null);
              }}
            >
              {t("pim-nav-today")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-variant="icon ghost"
              data-field="calendar-previous"
              aria-label={t("pim-nav-previous")}
              title={t("pim-nav-previous")}
              onClick={() => {
                goTo(stepAnchor(view, anchor, -1, options));
              }}
            >
              <Icon name="chevron-left" part="form.button.icon" />
            </button>
            <button
              type="button"
              data-part="form.button"
              data-variant="icon ghost"
              data-field="calendar-next"
              aria-label={t("pim-nav-next")}
              title={t("pim-nav-next")}
              onClick={() => {
                goTo(stepAnchor(view, anchor, 1, options));
              }}
            >
              <Icon name="chevron-right" part="form.button.icon" />
            </button>
          </div>
          <h2 data-part="calendar.title" aria-live="polite" title={title}>
            {title}
          </h2>
          {views.length > 1 && (
            <ViewSwitch
              views={views}
              view={view}
              menu={switchStyle === "menu" || narrowSwitch}
              onChange={(kind) => {
                pim.getState().setView(kind);
              }}
            />
          )}
          <button
            type="button"
            data-part="form.button"
            data-variant="primary"
            data-field="calendar-new-event"
            onClick={() => {
              openNewEvent(ctx, anchor, { hour: 9, minute: 0 }, tz);
            }}
          >
            <Icon name="plus" part="form.button.icon" />
            <span data-part="form.button.label">{t("pim-new-event")}</span>
          </button>
        </div>
      )}
      {instances.isError && <Problem message={problemOf(instances.error)} />}
      {body}
    </section>
  );
}
