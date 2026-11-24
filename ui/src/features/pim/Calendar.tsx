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
 *
 * R7 (ADR 0246): the toolbar has the time zone chip (C5); a second row
 * (`calendar.tools`) has quick create from text (S2) and the search (S9,
 * `/` focuses it). The
 * views place events in the zones of `viewZonesOf` (layout option, then
 * the preferences) and leave out declined invitations when the preference
 * says so. Narrow windows (M7): when the window's layout has no calendar
 * sidebar at its width (the narrow breakpoint of `layout.kdl`), a
 * "Calendars" button opens the mini-month, the calendar list and the tasks
 * due this week as a drawer; a time grid whose day columns would be
 * narrower than `MIN_DAY_COLUMN_PX` shows three days, then one; narrow and
 * touch layouts get a floating "New event" button instead of the
 * toolbar's.
 */
import type { CalendarDate } from "@internationalized/date";
import { useEffect, useEffectEvent, useId, useRef, useState } from "react";
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
import { CalendarListView } from "./CalendarList";
import { CalendarSearch } from "./CalendarSearch";
import { Problem, problemOf, useChangeContext } from "./common";
import { DueTasks } from "./DueTasks";
import { CalendarEmpty } from "./EmptyState";
import { useCalendarPreferences } from "./prefs";
import { QuickAdd } from "./QuickAdd";
import { MiniMonthView } from "./SmallWidgets";
import {
  useCalendarZones,
  useCoarsePointer,
  useElementWidth,
  useLayoutShowsSidebar,
} from "./viewHooks";
import {
  DEFAULT_NEW_EVENT_TIME,
  TIME_GRID_VIEWS,
  narrowFallback,
  shownInstances,
} from "./viewsModel";
import { ZoneChip } from "./ZoneChip";
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
      return t("common-time-range", {
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
          {t("common-view")}
        </label>
        <select
          id={id}
          data-part="form.select"
          data-field="calendar-view"
          data-width="content"
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
      aria-label={t("common-view")}
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

/**
 * The calendar sidebar as a drawer over the view (narrow windows): the
 * mini-month, the calendar list and the tasks due this week. Esc, the
 * close button or choosing a day closes it.
 */
function CalendarDrawer({
  id,
  tz,
  calendarIds,
  onClose,
}: {
  id: string;
  tz: string;
  calendarIds: readonly string[] | null;
  onClose: () => void;
}) {
  const t = useT();
  const ref = useRef<HTMLElement>(null);
  const anchor = usePim((s) => s.anchor);
  const opened = useRef(anchor);
  const close = useEffectEvent(onClose);
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>('[data-field="drawer-close"]')?.focus();
    // Esc inside the drawer closes it.
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape" || !(event.target instanceof Node)) return;
      if (!ref.current?.contains(event.target)) return;
      event.preventDefault();
      event.stopPropagation();
      close();
    };
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("keydown", onKey, true);
    };
  }, []);
  // Choosing a day in the mini-month shows it: the drawer gets out of the way.
  useEffect(() => {
    if (anchor !== opened.current) close();
  }, [anchor]);
  return (
    <aside ref={ref} id={id} data-part="calendar.drawer" aria-label={t("common-calendars")}>
      <div data-part="calendar.drawer.header">
        <h2 data-part="calendar.drawer.title">{t("common-calendars")}</h2>
        <button
          type="button"
          data-part="form.button"
          data-variant="icon ghost"
          data-field="drawer-close"
          aria-label={t("common-close")}
          title={t("common-close")}
          onClick={onClose}
        >
          <Icon name="x" part="form.button.icon" />
        </button>
      </div>
      <MiniMonthView tz={tz} calendarIds={calendarIds} />
      <CalendarListView asButtons={false} bySource />
      <DueTasks tz={tz} lists={null} />
    </aside>
  );
}

export function CalendarWidget({ node, nodeKey }: WidgetProps) {
  const t = useT();
  const format = useDateFormat();
  const locale = useLocale();
  const ctx = useChangeContext();
  const pim = usePimStore();
  const prefs = useCalendarPreferences();
  const zones = useCalendarZones(node);
  const tz = zones.zone;
  const zoneFixed =
    optionString(node, "time-zone", "") !== "" || optionString(node, "secondary-zone", "") !== "";
  const own = optionString(node, "view", "week");
  const override = usePim((s) => s.view);
  const chosen: ViewKind = override ?? (isViewKind(own) ? own : "week");
  const views = optionList(node, "views", VIEW_KINDS).filter(isViewKind);
  const calendarIds = optionList(node, "calendars", []);
  const shownIds = calendarIds.length > 0 ? calendarIds : null;
  const header = optionBool(node, "header", true);
  const switchStyle = optionString(node, "view-switch", "segmented");
  const hourStart = clampHour(optionNumber(node, "hour-start", 0), 0);
  const hourEnd = Math.max(hourStart + 1, clampHour(optionNumber(node, "hour-end", 24), 24));
  const anchor = useAnchor(tz);
  const sectionRef = useRef<HTMLElement>(null);
  const width = useElementWidth(sectionRef);
  // Narrow panes: a week whose day columns would get too narrow shows three days, then one.
  const chosenOptions = viewOptions(node, chosen);
  const fallback = narrowFallback(
    chosen,
    viewRange(chosen, anchor, locale, chosenOptions).count,
    width,
    zones.second ? 2 : 1,
  );
  const view: ViewKind = fallback?.view ?? chosen;
  const options = fallback ? { ...chosenOptions, days: fallback.days } : chosenOptions;
  const range = viewRange(view, anchor, locale, options);
  const instances = useInstances(rangeInstants(range, tz), shownIds, tz);
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
  const drawerMode = !useLayoutShowsSidebar(node);
  const coarse = useCoarsePointer();
  const floatingNew = drawerMode || coarse;
  const [drawerOpen, setDrawerOpen] = useState(false);
  const drawerId = useId();

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
  const newEvent = () => {
    openNewEvent(ctx, anchor, DEFAULT_NEW_EVENT_TIME, tz);
  };
  const list = shownInstances(instances.data ?? [], prefs);

  let body: React.ReactNode;
  if (TIME_GRID_VIEWS.includes(view)) {
    body = (
      <TimeGrid
        nodeKey={nodeKey}
        days={daysOf(range)}
        instances={list}
        calendars={calendars.data}
        tz={tz}
        secondaryTz={zones.second}
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
        empty={
          instances.isSuccess && (
            <CalendarEmpty
              view="agenda"
              range={range}
              tz={tz}
              calendarIds={shownIds}
              onToday={() => {
                pim.getState().setAnchor(null);
              }}
              onNew={newEvent}
              onDay={showDay}
            />
          )
        }
      />
    );
  } else {
    // The layout's hours win; else the working hours of the preferences.
    const workStart = clampHour(
      optionNumber(node, "work-start", Math.floor(prefs.work_hours.start_minute / 60)),
      0,
    );
    const workEnd = clampHour(
      optionNumber(node, "work-end", Math.ceil(prefs.work_hours.end_minute / 60)),
      24,
    );
    body = (
      <FreeTimeView
        nodeKey={nodeKey}
        range={range}
        instances={list}
        tz={tz}
        minutes={Math.max(5, Math.trunc(optionNumber(node, "duration", 30)))}
        dayStartHour={workStart}
        dayEndHour={Math.max(workStart + 1, workEnd)}
      />
    );
  }

  return (
    <section
      ref={sectionRef}
      data-part="calendar"
      data-view={view}
      data-fallback={fallback ? chosen : undefined}
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
          {drawerMode && (
            <button
              type="button"
              data-part="form.button"
              data-variant="icon ghost"
              data-field="calendar-drawer"
              aria-label={t("pim-views-drawer-open")}
              title={t("pim-views-drawer-open")}
              aria-expanded={drawerOpen}
              aria-controls={drawerOpen ? drawerId : undefined}
              onClick={() => {
                setDrawerOpen((open) => !open);
              }}
            >
              <Icon name="panel-left" part="form.button.icon" />
            </button>
          )}
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
              {t("common-today")}
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
              aria-label={t("common-next-period")}
              title={t("common-next-period")}
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
              view={chosen}
              menu={switchStyle === "menu" || narrowSwitch}
              onChange={(kind) => {
                pim.getState().setView(kind);
              }}
            />
          )}
          <ZoneChip zones={zones} fixed={zoneFixed} />
          {!floatingNew && (
            <button
              type="button"
              data-part="form.button"
              data-variant="primary"
              data-field="calendar-new-event"
              onClick={newEvent}
            >
              <Icon name="plus" part="form.button.icon" />
              <span data-part="form.button.label">{t("pim-action-new-event")}</span>
            </button>
          )}
        </div>
      )}
      {header && (
        <div data-part="calendar.tools">
          <QuickAdd day={anchor} tz={tz} />
          <CalendarSearch tz={tz} calendars={calendars.data} />
        </div>
      )}
      {drawerMode && drawerOpen && (
        <CalendarDrawer
          id={drawerId}
          tz={tz}
          calendarIds={shownIds}
          onClose={() => {
            setDrawerOpen(false);
          }}
        />
      )}
      {floatingNew && (
        <button
          type="button"
          data-part="calendar.fab"
          data-field="calendar-new-event"
          aria-label={t("pim-action-new-event")}
          title={t("pim-action-new-event")}
          onClick={newEvent}
        >
          <Icon name="plus" part="calendar.fab.icon" />
        </button>
      )}
      {instances.isError && <Problem message={problemOf(instances.error)} />}
      {body}
    </section>
  );
}
