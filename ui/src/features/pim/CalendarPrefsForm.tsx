/**
 * The calendar preferences in Settings → Calendar & contacts (S6, ADR
 * 0246): working hours, work days, the first day of the week, week
 * numbers and whether declined invitations show. R6 (ADR 0239): the edits
 * wait for the settings page's Save (`useSettingsForm`, the pinned save
 * bar); the core checks them (`SetCalendarPreferences`) and announces the
 * change to every view.
 *
 * Parts: `card` (`data-field="calendar-preferences"`), `calendar-prefs`
 * (the form), `calendar-prefs.hours`, `calendar-prefs.days`, `form.field`
 * / `form.select` / `form.check`, `calendar-prefs.problem`.
 */
import { useId, useRef } from "react";
import type { CalendarPreferences, Weekday } from "../../api/generated";
import { MINUTES_PER_DAY } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { useNow } from "../../state/clock";
import { useFormNotice, useSettingsForm } from "../settings/draft";
import { Problem, useChangeContext } from "./common";
import { formatMinutes, weekdayName } from "./fmt";
import { prefsKeys, saveCalendarPreferences, useCalendarPreferences, weekdayOf } from "./prefs";
import { WEEKDAYS, localZone, startOfWeekFor, todayIn } from "./time";

/** The steps the working hours are chosen in (minutes). */
export const WORK_HOUR_STEP = 30;

/** The times of day offered for the working hours, `[0, MINUTES_PER_DAY]` in steps. */
export function workHourChoices(): number[] {
  return Array.from({ length: MINUTES_PER_DAY / WORK_HOUR_STEP + 1 }, (_, i) => i * WORK_HOUR_STEP);
}

/** Whether the preferences can be saved (the core's working-hours rule). */
export function prefsValid(prefs: CalendarPreferences): boolean {
  const { start_minute, end_minute } = prefs.work_hours;
  return start_minute < end_minute && end_minute <= MINUTES_PER_DAY;
}

/** `days` with `day` added or taken out, in week order. */
export function toggleWeekday(days: readonly Weekday[], day: Weekday, on: boolean): Weekday[] {
  const set = new Set(days);
  if (on) set.add(day);
  else set.delete(day);
  return WEEKDAYS.filter((d) => set.has(d));
}

export function CalendarPrefsForm() {
  const t = useT();
  const format = useDateFormat();
  const locale = useLocale();
  const ctx = useChangeContext();
  const saved = useCalendarPreferences();
  const notice = useFormNotice();
  const ids = { title: useId(), start: useId(), end: useId(), weekStart: useId() };
  const titleRef = useRef<HTMLHeadingElement>(null);
  const form = useSettingsForm<CalendarPreferences>(saved, {
    canSave: prefsValid,
    save: async (prefs) => {
      const written = await saveCalendarPreferences(ctx, prefs);
      ctx.queryClient.setQueryData(prefsKeys.preferences, written);
    },
    ...notice.outcome("pim-views-prefs-saved"),
  });
  const prefs = form.value;
  const set = (change: Partial<CalendarPreferences>) => {
    notice.clear();
    form.set((current) => ({ ...current, ...change }));
  };
  const choices = workHourChoices();
  const localeStart = weekdayOf(startOfWeekFor(todayIn(localZone(), useNow()), locale));
  const invalid = !prefsValid(prefs);
  return (
    <section data-part="card" data-field="calendar-preferences" aria-labelledby={ids.title}>
      <h3 data-part="card.title" id={ids.title} ref={titleRef} tabIndex={-1}>
        {t("pim-views-prefs-title")}
      </h3>
      <form
        data-part="calendar-prefs"
        noValidate
        onSubmit={(e) => {
          e.preventDefault();
        }}
      >
        <div data-part="calendar-prefs.hours" role="group" aria-label={t("pim-views-prefs-hours")}>
          <div data-part="form.field" data-field="work-start">
            <label data-part="form.label" htmlFor={ids.start}>
              {t("pim-views-prefs-start")}
            </label>
            <select
              id={ids.start}
              data-part="form.select"
              data-width="short"
              value={prefs.work_hours.start_minute}
              aria-invalid={invalid || undefined}
              onChange={(e) => {
                set({ work_hours: { ...prefs.work_hours, start_minute: Number(e.target.value) } });
              }}
            >
              {choices.slice(0, -1).map((m) => (
                <option key={m} value={m}>
                  {formatMinutes(format, m)}
                </option>
              ))}
            </select>
          </div>
          <div data-part="form.field" data-field="work-end">
            <label data-part="form.label" htmlFor={ids.end}>
              {t("pim-views-prefs-end")}
            </label>
            <select
              id={ids.end}
              data-part="form.select"
              data-width="short"
              value={prefs.work_hours.end_minute}
              aria-invalid={invalid || undefined}
              onChange={(e) => {
                set({ work_hours: { ...prefs.work_hours, end_minute: Number(e.target.value) } });
              }}
            >
              {choices.slice(1).map((m) => (
                <option key={m} value={m}>
                  {m === MINUTES_PER_DAY ? t("pim-views-prefs-midnight") : formatMinutes(format, m)}
                </option>
              ))}
            </select>
          </div>
        </div>
        {invalid && (
          <Problem
            message={{ id: "pim-views-prefs-hours-invalid" }}
            part="calendar-prefs.problem"
          />
        )}
        <fieldset data-part="calendar-prefs.days" data-field="work-days">
          <legend data-part="form.label">{t("pim-views-prefs-days")}</legend>
          {WEEKDAYS.map((day) => (
            <label key={day} data-part="form.check" data-day={day}>
              <input
                type="checkbox"
                checked={prefs.work_days.includes(day)}
                onChange={(e) => {
                  set({ work_days: toggleWeekday(prefs.work_days, day, e.target.checked) });
                }}
              />
              <span>{weekdayName(format, day, "long")}</span>
            </label>
          ))}
        </fieldset>
        <div data-part="form.field" data-field="week-start">
          <label data-part="form.label" htmlFor={ids.weekStart}>
            {t("pim-views-prefs-week-start")}
          </label>
          <select
            id={ids.weekStart}
            data-part="form.select"
            data-width="medium"
            value={prefs.week_start ?? ""}
            onChange={(e) => {
              const value = e.target.value;
              set({ week_start: WEEKDAYS.find((d) => d === value) ?? null });
            }}
          >
            <option value="">
              {t("pim-views-prefs-week-start-locale", { day: weekdayName(format, localeStart) })}
            </option>
            {WEEKDAYS.map((day) => (
              <option key={day} value={day}>
                {weekdayName(format, day, "long")}
              </option>
            ))}
          </select>
        </div>
        <label data-part="form.check" data-field="week-numbers">
          <input
            type="checkbox"
            checked={prefs.show_week_numbers}
            onChange={(e) => {
              set({ show_week_numbers: e.target.checked });
            }}
          />
          <span>{t("pim-views-prefs-week-numbers")}</span>
        </label>
        <label data-part="form.check" data-field="show-declined">
          <input
            type="checkbox"
            checked={prefs.show_declined}
            onChange={(e) => {
              set({ show_declined: e.target.checked });
            }}
          />
          <span>{t("pim-views-prefs-show-declined")}</span>
        </label>
        {notice.notice && (
          <p data-part="calendar-prefs.notice" role="status">
            {t(notice.notice)}
          </p>
        )}
        {notice.problems([]).map((problem) => (
          <Problem key={problem.id} message={problem} part="calendar-prefs.problem" />
        ))}
      </form>
    </section>
  );
}
