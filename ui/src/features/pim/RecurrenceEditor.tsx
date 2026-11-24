/**
 * The recurrence editor of the event and task editors: repeat never /
 * daily / weekly (on chosen weekdays) / monthly (on the day or on the n-th
 * weekday) / yearly, every n periods, ending never / after n times / on a
 * day, with a readable summary. A rule the simple editor cannot show is
 * kept as it is ("custom") until the user replaces it.
 *
 * R7 (S3, ADR 0246): the pieces stand alone too — `RepeatSelect` (the
 * "Repeats" choice, a chip in the compact event editor) and
 * `RecurrenceDetails` (every n, weekdays, the end, the summary), shown
 * below the chips only while the event repeats.
 */
import type { CalendarDate } from "@internationalized/date";
import { useId } from "react";
import type { PimTime } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { NumberField } from "./common";
import { DateTimeInput } from "./DateTimeInput";
import { formatDay, weekdayName } from "./fmt";
import {
  SIMPLE_FREQUENCIES,
  defaultRule,
  describe,
  sortWeekdays,
  weekdayOf,
  weekdayOrdinal,
  type RecurrenceChoice,
  type SimpleFrequency,
  type SimpleRule,
} from "./recurrence";
import { WEEKDAYS, splitPimTime } from "./time";

export interface RecurrenceEditorProps {
  value: RecurrenceChoice;
  onChange: (value: RecurrenceChoice) => void;
  /** The series' start (the rule's anchor). */
  start: PimTime;
  disabled?: boolean;
  tz: string;
}

export interface RecurrenceEditorProps {
  value: RecurrenceChoice;
  onChange: (value: RecurrenceChoice) => void;
  /** The series' start (the rule's anchor). */
  start: PimTime;
  disabled?: boolean;
  tz: string;
}

/** The readable summary of a choice ("Every week on Friday."). */
function useSummary(value: RecurrenceChoice, start: PimTime): string {
  const t = useT();
  const format = useDateFormat();
  if (value.type === "simple") {
    const d = describe(
      value.rule,
      start,
      (day: CalendarDate) => formatDay(format, day),
      (day) => weekdayName(format, day, "long"),
    );
    return t(d.id, d.args);
  }
  return value.type === "custom" ? t("pim-recurrence-custom") : t("pim-recurrence-none");
}

export interface RepeatSelectProps {
  value: RecurrenceChoice;
  onChange: (value: RecurrenceChoice) => void;
  start: PimTime;
  disabled?: boolean;
  /** The label for screen readers only (the compact editor's chip). */
  labelHidden?: boolean;
  /** Part of the field (default `form.field`). */
  part?: string;
  /** Part of the select (default `form.select`). */
  selectPart?: string;
}

/** "Repeats": never, daily, weekly, monthly, yearly (or the kept custom rule). */
export function RepeatSelect({
  value,
  onChange,
  start,
  disabled = false,
  labelHidden = false,
  part = "form.field",
  selectPart = "form.select",
}: RepeatSelectProps) {
  const t = useT();
  const id = useId();
  const startDay = splitPimTime(start).day;
  const freqValue =
    value.type === "none" ? "none" : value.type === "custom" ? "custom" : value.rule.freq;
  return (
    <div data-part={part} data-field="freq">
      <label data-part={labelHidden ? "pim.look.hidden" : "form.label"} htmlFor={id}>
        {t("common-repeats")}
      </label>
      <select
        id={id}
        data-part={selectPart}
        value={freqValue}
        disabled={disabled}
        onChange={(event) => {
          const next = event.target.value;
          if (next === "none") onChange({ type: "none" });
          else if (next === "custom") return;
          else onChange({ type: "simple", rule: defaultRule(next as SimpleFrequency, startDay) });
        }}
      >
        <option value="none">{t("pim-recurrence-freq-none")}</option>
        {SIMPLE_FREQUENCIES.map((f) => (
          <option key={f} value={f}>
            {t(`pim-recurrence-freq-${f}`)}
          </option>
        ))}
        {value.type === "custom" && (
          <option value="custom">{t("pim-recurrence-freq-custom")}</option>
        )}
      </select>
    </div>
  );
}

/** How a repeating event repeats: every n, weekdays, monthly mode, the end and the summary. */
export function RecurrenceDetails({
  value,
  onChange,
  start,
  disabled = false,
  tz,
}: RecurrenceEditorProps) {
  const t = useT();
  const format = useDateFormat();
  const ids = { freq: useId(), interval: useId(), count: useId(), summary: useId() };
  const startDay = splitPimTime(start).day;
  const summary = useSummary(value, start);
  const setRule = (patch: Partial<SimpleRule>) => {
    if (value.type !== "simple") return;
    onChange({ type: "simple", rule: { ...value.rule, ...patch } });
  };
  return (
    <fieldset
      data-part="pim.recurrence.details"
      disabled={disabled}
      aria-label={t("pim-recurrence-legend")}
      aria-describedby={ids.summary}
    >
      {value.type === "simple" && (
        <>
          <div data-part="form.field" data-field="interval">
            <label data-part="form.label" htmlFor={ids.interval}>
              {t("pim-recurrence-every", { freq: value.rule.freq })}
            </label>
            <NumberField
              id={ids.interval}
              value={value.rule.interval}
              min={1}
              max={999}
              onChange={(interval) => {
                setRule({ interval });
              }}
            />
          </div>

          {value.rule.freq === "weekly" && (
            <fieldset data-part="pim.recurrence.weekdays">
              <legend data-part="form.label">{t("common-on")}</legend>
              {WEEKDAYS.map((day) => {
                const checked = value.rule.weekdays.includes(day);
                return (
                  <label key={day} data-part="form.check" data-day={day}>
                    <input
                      type="checkbox"
                      checked={checked}
                      onChange={() => {
                        const next = checked
                          ? value.rule.weekdays.filter((d) => d !== day)
                          : [...value.rule.weekdays, day];
                        setRule({
                          weekdays: next.length > 0 ? sortWeekdays(next) : [weekdayOf(startDay)],
                        });
                      }}
                    />
                    <span>{weekdayName(format, day, "short")}</span>
                  </label>
                );
              })}
            </fieldset>
          )}

          {(value.rule.freq === "monthly" || value.rule.freq === "yearly") && (
            <fieldset data-part="pim.recurrence.monthly">
              <legend data-part="form.label">{t("pim-recurrence-monthly-legend")}</legend>
              <label data-part="form.check">
                <input
                  type="radio"
                  name={`${ids.freq}-monthly`}
                  checked={value.rule.monthly === "day"}
                  onChange={() => {
                    setRule({ monthly: "day" });
                  }}
                />
                <span>{t("pim-recurrence-monthly-day", { day: startDay.day })}</span>
              </label>
              <label data-part="form.check">
                <input
                  type="radio"
                  name={`${ids.freq}-monthly`}
                  checked={value.rule.monthly === "weekday"}
                  onChange={() => {
                    setRule({ monthly: "weekday" });
                  }}
                />
                <span>
                  {t("pim-recurrence-monthly-weekday", {
                    ordinal: (() => {
                      const { ordinal, last } = weekdayOrdinal(startDay);
                      return last && ordinal > 4 ? -1 : ordinal;
                    })(),
                    weekday: weekdayName(format, weekdayOf(startDay), "long"),
                  })}
                </span>
              </label>
            </fieldset>
          )}

          <fieldset data-part="pim.recurrence.end">
            <legend data-part="form.label">{t("pim-recurrence-ends")}</legend>
            <label data-part="form.check">
              <input
                type="radio"
                name={`${ids.freq}-end`}
                checked={value.rule.end.type === "never"}
                onChange={() => {
                  setRule({ end: { type: "never" } });
                }}
              />
              <span>{t("common-never")}</span>
            </label>
            <label data-part="form.check">
              <input
                type="radio"
                name={`${ids.freq}-end`}
                checked={value.rule.end.type === "count"}
                onChange={() => {
                  setRule({ end: { type: "count", count: 10 } });
                }}
              />
              <span>{t("pim-recurrence-end-count")}</span>
            </label>
            {value.rule.end.type === "count" && (
              <div data-part="form.field" data-field="count">
                <label data-part="form.label" htmlFor={ids.count}>
                  {t("pim-recurrence-count")}
                </label>
                <NumberField
                  id={ids.count}
                  value={value.rule.end.count}
                  min={1}
                  max={9999}
                  onChange={(count) => {
                    setRule({ end: { type: "count", count } });
                  }}
                />
              </div>
            )}
            <label data-part="form.check">
              <input
                type="radio"
                name={`${ids.freq}-end`}
                checked={value.rule.end.type === "until"}
                onChange={() => {
                  setRule({ end: { type: "until", until: startDay.add({ months: 3 }) } });
                }}
              />
              <span>{t("pim-recurrence-end-until")}</span>
            </label>
            {value.rule.end.type === "until" && (
              <DateTimeInput
                label={t("pim-recurrence-until")}
                field="until"
                day={value.rule.end.until}
                time={null}
                tz={tz}
                onChange={(day) => {
                  setRule({ end: { type: "until", until: day } });
                }}
              />
            )}
          </fieldset>
        </>
      )}

      {value.type === "custom" && (
        <button
          type="button"
          data-part="form.button"
          onClick={() => {
            onChange({ type: "simple", rule: defaultRule("weekly", startDay) });
          }}
        >
          {t("pim-recurrence-replace")}
        </button>
      )}

      <p id={ids.summary} data-part="pim.recurrence.summary" aria-live="polite">
        {summary}
      </p>
    </fieldset>
  );
}

/** The whole recurrence editor (the task editor's): the choice and its details. */
export function RecurrenceEditor({
  value,
  onChange,
  start,
  disabled = false,
  tz,
}: RecurrenceEditorProps) {
  const t = useT();
  return (
    <fieldset data-part="pim.recurrence" disabled={disabled}>
      <legend data-part="form.section.title">{t("pim-recurrence-legend")}</legend>
      <RepeatSelect value={value} onChange={onChange} start={start} disabled={disabled} />
      <RecurrenceDetails
        value={value}
        onChange={onChange}
        start={start}
        disabled={disabled}
        tz={tz}
      />
    </fieldset>
  );
}
