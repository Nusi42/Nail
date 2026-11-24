/**
 * Empty states of the calendar and the tasks (C4, ADR 0246): an icon, a
 * sentence and the next steps instead of one line. An empty calendar range
 * offers "Back to today" and "New event" and names the nearest event
 * outside the range (one extra `ListEventInstances` query each way, at most
 * a year, only while the range is empty).
 *
 * Parts: `pim.empty` (`data-empty`: the view), `pim.empty.icon`,
 * `pim.empty.text`, `pim.empty.actions`, `pim.empty.nearest`.
 */
import type { ReactNode } from "react";
import type { CalendarId } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { Icon } from "../../widgets/Icon";
import { formatPimTime } from "./fmt";
import { useInstances } from "./queries";
import {
  pimTimeIn,
  rangeInstants,
  toCalendarDate,
  type CalendarDate,
  type ViewRange,
} from "./time";
import { nearestOutside } from "./viewsModel";

/** The year before and the year after `range` (where the nearest event is looked for). */
export function yearsAround(range: ViewRange): { before: ViewRange; after: ViewRange } {
  const yearBack = range.start.add({ years: -1 });
  const end = range.start.add({ days: range.count });
  return {
    before: { start: yearBack, count: range.start.compare(yearBack) },
    after: { start: end, count: end.add({ years: 1 }).compare(end) },
  };
}

/** An icon, a sentence and actions. */
export function EmptyState({
  view,
  icon,
  text,
  children,
}: {
  view: string;
  icon: string;
  text: string;
  children?: ReactNode;
}) {
  return (
    <div data-part="pim.empty" data-empty={view} role="status">
      <Icon name={icon} part="pim.empty.icon" />
      <p data-part="pim.empty.text">{text}</p>
      {children}
    </div>
  );
}

/** The empty state of a calendar range: Back to today, New event and the nearest event. */
export function CalendarEmpty({
  view,
  range,
  tz,
  calendarIds,
  onToday,
  onNew,
  onDay,
}: {
  view: string;
  range: ViewRange;
  tz: string;
  calendarIds: readonly CalendarId[] | null;
  onToday: () => void;
  onNew: () => void;
  onDay: (day: CalendarDate) => void;
}) {
  const t = useT();
  const format = useDateFormat();
  const around = yearsAround(range);
  const before = useInstances(rangeInstants(around.before, tz), calendarIds, tz);
  const after = useInstances(rangeInstants(around.after, tz), calendarIds, tz);
  const nearest = nearestOutside(before.data ?? [], after.data ?? []);
  return (
    <EmptyState view={view} icon="calendar" text={t("pim-views-empty-range")}>
      <div data-part="pim.empty.actions">
        <button type="button" data-part="form.button" data-action="back-to-today" onClick={onToday}>
          {t("pim-views-empty-today")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          data-action="empty-new-event"
          onClick={onNew}
        >
          <Icon name="plus" part="form.button.icon" />
          <span data-part="form.button.label">{t("pim-action-new-event")}</span>
        </button>
      </div>
      {nearest && (
        <button
          type="button"
          data-part="pim.empty.nearest"
          data-side={nearest.side}
          onClick={() => {
            onDay(toCalendarDate(pimTimeIn(nearest.instance.start, tz)));
          }}
        >
          {t(nearest.side === "after" ? "pim-views-empty-next" : "pim-views-empty-previous", {
            title: nearest.instance.title || t("common-no-title"),
            when: formatPimTime(format, nearest.instance.start, tz),
          })}
        </button>
      )}
    </EmptyState>
  );
}
