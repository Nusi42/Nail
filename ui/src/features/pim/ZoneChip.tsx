/**
 * The time zone chip of the calendar toolbar (C5, ADR 0246): it names the
 * zone the views show (and the second zone of the time grid's gutter, if
 * any); choosing it opens a popover to switch the view zone or add, change
 * or remove the second zone. The choices are the calendar preferences
 * (`SetCalendarPreferences`); a widget's `time-zone` / `secondary-zone`
 * layout options still win for that widget, which the popover says.
 *
 * Parts: `calendar.zone-chip` (`data-field="calendar-zone"`), `.label`,
 * the popover `calendar.zone-popover` with `form.field` / `form.select`
 * (`data-field="view-zone"`, `second-zone"`), `calendar.zone-popover.hint`.
 */
import { useId, useRef, useState } from "react";
import type { CalendarPreferences, LocalizedMessage } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { useNow } from "../../state/clock";
import { anchorOf, Popover, type AnchorRect } from "../../widgets/Popover";
import { Icon } from "../../widgets/Icon";
import { Problem, problemOf, useChangeContext } from "./common";
import { zoneLabel } from "./fmt";
import { saveCalendarPreferences, useCalendarPreferences, type ViewZones } from "./prefs";
import { localZone, timeZoneNames } from "./time";

export function ZoneChip({ zones, fixed }: { zones: ViewZones; fixed: boolean }) {
  const t = useT();
  const format = useDateFormat();
  const now = useNow();
  const button = useRef<HTMLButtonElement>(null);
  const [anchor, setAnchor] = useState<AnchorRect | null>(null);
  const main = zoneLabel(format, now, zones.zone);
  const text = zones.second
    ? t("pim-views-zone-chip-two", { zone: main, second: zoneLabel(format, now, zones.second) })
    : main;
  return (
    <>
      <button
        ref={button}
        type="button"
        data-part="calendar.zone-chip"
        data-field="calendar-zone"
        aria-haspopup="dialog"
        aria-expanded={anchor !== null}
        aria-label={t("pim-views-zone-chip-label", {
          zone: zones.zone,
          second: zones.second ?? "none",
        })}
        title={t("pim-views-zone-chip-label", { zone: zones.zone, second: zones.second ?? "none" })}
        onClick={() => {
          setAnchor((a) => (a === null && button.current ? anchorOf(button.current) : null));
        }}
      >
        <Icon name="globe" part="calendar.zone-chip.icon" />
        <span data-part="calendar.zone-chip.label">{text}</span>
      </button>
      {anchor !== null && (
        <Popover
          anchor={anchor}
          placement="below"
          label={t("pim-views-zone-title")}
          part="calendar.zone-popover"
          onClose={() => {
            setAnchor(null);
          }}
        >
          <ZoneForm fixed={fixed} />
        </Popover>
      )}
    </>
  );
}

/** One zone choice: `""` (the first option, `empty`) or a zone name. */
function ZoneSelect({
  field,
  label,
  empty,
  value,
  autofocus = false,
  onChange,
}: {
  field: string;
  label: string;
  empty: string;
  value: string | null;
  autofocus?: boolean;
  onChange: (zone: string | null) => void;
}) {
  const id = useId();
  return (
    <div data-part="form.field" data-field={field}>
      <label data-part="form.label" htmlFor={id}>
        {label}
      </label>
      <select
        id={id}
        data-part="form.select"
        data-autofocus={autofocus || undefined}
        value={value ?? ""}
        onChange={(e) => {
          onChange(e.target.value || null);
        }}
      >
        <option value="">{empty}</option>
        {timeZoneNames().map((zone) => (
          <option key={zone} value={zone}>
            {zone}
          </option>
        ))}
      </select>
    </div>
  );
}

function ZoneForm({ fixed }: { fixed: boolean }) {
  const t = useT();
  const ctx = useChangeContext();
  const prefs = useCalendarPreferences();
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const save = async (change: Partial<CalendarPreferences>) => {
    setProblem(null);
    try {
      await saveCalendarPreferences(ctx, { ...prefs, ...change });
    } catch (error) {
      setProblem(problemOf(error));
    }
  };
  return (
    <div data-part="calendar.zone-popover.body">
      <h3 data-part="calendar.zone-popover.title">{t("pim-views-zone-title")}</h3>
      <ZoneSelect
        field="view-zone"
        label={t("pim-views-zone-view")}
        empty={t("pim-views-zone-system", { zone: localZone() })}
        value={prefs.time_zone}
        autofocus
        onChange={(zone) => void save({ time_zone: zone })}
      />
      <ZoneSelect
        field="second-zone"
        label={t("pim-views-zone-second")}
        empty={t("pim-views-zone-none")}
        value={prefs.second_time_zone}
        onChange={(zone) => void save({ second_time_zone: zone })}
      />
      {fixed && <p data-part="calendar.zone-popover.hint">{t("pim-views-zone-fixed")}</p>}
      <Problem message={problem} />
    </div>
  );
}
