/**
 * The accessible date / time picker of the PIM editors.
 *
 * Native `<input type="date|time">` follow the browser's or the system's
 * locale, not Nail's UI language, so this field uses React Aria's segmented
 * `DateField` / `TimeField` (each part a spin button, typed or changed with
 * the arrow keys) under an `I18nProvider` set to the UI locale: the order of
 * day / month / year, the 12- or 24-hour clock and the segment names follow
 * the UI language. A button opens a month grid (`MonthGrid`) to pick the day
 * with the mouse or the keyboard. Every element carries a `data-part`; the
 * theme styles them (`pim.datetime.*`).
 */
import { Time, type CalendarDate } from "@internationalized/date";
import { useEffect, useId, useRef, useState } from "react";
import {
  DateField,
  DateInput,
  DateSegment,
  I18nProvider as AriaI18nProvider,
  TimeField,
} from "react-aria-components";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { Icon } from "../../widgets/Icon";
import { MonthGrid } from "./MonthGrid";
import { todayIn } from "./time";

export interface Clock {
  hour: number;
  minute: number;
}

export interface DateTimeInputProps {
  /** Visible label of the whole field. */
  label: string;
  day: CalendarDate;
  /** `null` = date only. */
  time: Clock | null;
  onChange(day: CalendarDate, time: Clock | null): void;
  /** Field name (`data-field`), e.g. `start`. */
  field: string;
  disabled?: boolean;
  invalid?: boolean;
  /** Id of an element describing the field (an error text). */
  describedBy?: string | undefined;
  /** Zone used for "today" in the picker. */
  tz: string;
}

export function DateTimeInput({
  label,
  day,
  time,
  onChange,
  field,
  disabled = false,
  invalid = false,
  describedBy,
  tz,
}: DateTimeInputProps) {
  const t = useT();
  const locale = useLocale();
  const labelId = useId();
  const popupId = useId();
  const [open, setOpen] = useState(false);
  const [focused, setFocused] = useState<CalendarDate>(day);
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);

  // Clicking outside closes the popup.
  useEffect(() => {
    if (!open) return;
    const onDown = (event: PointerEvent) => {
      if (event.target instanceof Node && rootRef.current?.contains(event.target)) return;
      setOpen(false);
    };
    document.addEventListener("pointerdown", onDown);
    return () => document.removeEventListener("pointerdown", onDown);
  }, [open]);

  const close = () => {
    setOpen(false);
    buttonRef.current?.focus();
  };

  const extra = describedBy ? { "aria-describedby": describedBy } : {};

  return (
    <div
      ref={rootRef}
      data-part="pim.datetime"
      data-field={field}
      data-state={
        [invalid ? "invalid" : null, open ? "open" : null].filter(Boolean).join(" ") || undefined
      }
      role="group"
      aria-labelledby={labelId}
    >
      <span id={labelId} data-part="pim.datetime.label">
        {label}
      </span>
      <AriaI18nProvider locale={locale}>
        <div data-part="pim.datetime.controls">
          <DateField
            data-part="pim.datetime.date"
            value={day}
            granularity="day"
            isDisabled={disabled}
            isInvalid={invalid}
            aria-label={t("pim-datetime-date", { field: label })}
            onChange={(value) => {
              if (value) onChange(value, time);
            }}
            {...extra}
          >
            <DateInput data-part="pim.datetime.input">
              {(segment) => <DateSegment segment={segment} data-part="pim.datetime.segment" />}
            </DateInput>
          </DateField>
          <button
            ref={buttonRef}
            type="button"
            data-part="pim.datetime.pick"
            aria-haspopup="dialog"
            aria-expanded={open}
            aria-controls={open ? popupId : undefined}
            aria-label={t("pim-datetime-pick", { field: label })}
            title={t("pim-datetime-pick", { field: label })}
            disabled={disabled}
            onClick={() => {
              setFocused(day);
              setOpen((o) => !o);
            }}
          >
            <Icon name="calendar" part="pim.datetime.pick.icon" />
            <span data-part="pim.datetime.pick.label">{t("pim-datetime-pick-short")}</span>
          </button>
          {time !== null && (
            <TimeField
              data-part="pim.datetime.time"
              value={new Time(time.hour, time.minute)}
              isDisabled={disabled}
              isInvalid={invalid}
              aria-label={t("pim-datetime-time", { field: label })}
              onChange={(value) => {
                if (value) onChange(day, { hour: value.hour, minute: value.minute });
              }}
              {...extra}
            >
              <DateInput data-part="pim.datetime.input">
                {(segment) => <DateSegment segment={segment} data-part="pim.datetime.segment" />}
              </DateInput>
            </TimeField>
          )}
        </div>
      </AriaI18nProvider>
      {open && (
        <div
          id={popupId}
          role="dialog"
          data-part="pim.datetime.popup"
          aria-label={t("pim-datetime-pick", { field: label })}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              close();
            }
          }}
        >
          <div data-part="pim.datetime.popup.nav">
            <button
              type="button"
              data-part="pim.datetime.popup.previous"
              aria-label={t("pim-month-previous")}
              title={t("pim-month-previous")}
              onClick={() => setFocused((f) => f.add({ months: -1 }))}
            >
              <Icon name="chevron-left" part="pim.datetime.popup.icon" />
              <span data-part="pim.nav.text">{t("pim-nav-previous-short")}</span>
            </button>
            <button
              type="button"
              data-part="pim.datetime.popup.next"
              aria-label={t("pim-month-next")}
              title={t("pim-month-next")}
              onClick={() => setFocused((f) => f.add({ months: 1 }))}
            >
              <Icon name="chevron-right" part="pim.datetime.popup.icon" />
              <span data-part="pim.nav.text">{t("pim-nav-next-short")}</span>
            </button>
          </div>
          <MonthGrid
            part="pim.datetime.month"
            month={focused}
            focused={focused}
            selected={day}
            today={todayIn(tz)}
            onFocusDay={setFocused}
            onSelect={(picked) => {
              onChange(picked, time);
              close();
            }}
            autoFocus
          />
        </div>
      )}
    </div>
  );
}
