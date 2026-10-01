/**
 * The event editor (dialog `pim-event-editor`): title, calendar, all-day,
 * start / end with the locale-following date / time picker, the time zone
 * (or "floating"), a different zone for the end, a secondary zone shown
 * next to the times, location, conferencing links, description, the
 * recurrence editor, attendees (with suggestions from the address books),
 * alarms, status / busy / visibility.
 *
 * Editing one occurrence of a recurring event asks what to change: "this
 * occurrence", "this and following" (the core splits the series) or "all
 * occurrences" (moving the times moves the whole series by the same
 * amount). Deleting uses the same choice.
 */
import type { CalendarDate } from "@internationalized/date";
import { useId, useMemo, useState, type FormEvent } from "react";
import type {
  Alarm,
  AlarmAction,
  Attendee,
  Calendar,
  CalendarEvent,
  ConferenceLink,
  EditScope,
  EventFields,
  EventStatus,
  LocalizedMessage,
  PimTime,
  Transparency,
  Visibility,
} from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import {
  ALARM_ACTIONS,
  ALARM_UNITS,
  EDIT_SCOPES,
  EVENT_STATUSES,
  TRANSPARENCIES,
  VISIBILITIES,
  type AlarmUnit,
} from "./constants";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { wallDelta } from "./changes";
import { NumberField, Problem, problemOf, useChangeContext } from "./common";
import { DateTimeInput, type Clock } from "./DateTimeInput";
import { formatInstant } from "./fmt";
import { deleteEvent, saveEvent, useCalendars, useEvent } from "./queries";
import { AttendeePicker, MeetingPanel } from "../scheduling";
import { useMyAddresses } from "../scheduling/queries";
import { RecurrenceEditor } from "./RecurrenceEditor";
import { choiceOf, recurrenceOf, type RecurrenceChoice } from "./recurrence";
import { emptyEventFields } from "./defaults";
import { usePim, type EventEditorTarget } from "./store";
import {
  localZone,
  makePimTime,
  pimTimeIn,
  sameTime,
  shiftPimTime,
  splitPimTime,
  timeZoneNames,
} from "./time";

const UNIT_SECONDS: Record<AlarmUnit, number> = {
  minutes: 60,
  hours: 3600,
  days: 86_400,
  weeks: 604_800,
};
export {
  ALARM_UNITS,
  ATTENDEE_ROLES,
  EDIT_SCOPES,
  EVENT_STATUSES,
  TRANSPARENCIES,
  VISIBILITIES,
} from "./constants";

/** The editor's form. */
export interface EventForm {
  calendarId: string;
  title: string;
  allDay: boolean;
  startDay: CalendarDate;
  startTime: Clock;
  /** All-day: the last day (inclusive). */
  endDay: CalendarDate;
  endTime: Clock;
  /** `null` = floating. */
  tz: string | null;
  /** A different zone for the end, `null` = the start's. */
  endTz: string | null;
  location: string;
  description: string;
  url: string;
  status: EventStatus;
  transparency: Transparency;
  visibility: Visibility;
  conferences: ConferenceLink[];
  attendees: Attendee[];
  alarms: Alarm[];
  recurrence: RecurrenceChoice;
  scope: EditScope;
}

/** The form for fields (and the series' recurrence). */
export function formOf(
  fields: EventFields,
  calendarId: string,
  recurrence: CalendarEvent["recurrence"],
  seriesStart: PimTime,
  fallbackTz: string,
  scope: EditScope,
): EventForm {
  const start = splitPimTime(fields.start);
  const end = splitPimTime(fields.end);
  const allDay = fields.start.type === "date";
  const endDay = allDay ? end.day.add({ days: -1 }) : end.day;
  return {
    calendarId,
    title: fields.title,
    allDay,
    startDay: start.day,
    startTime: start.time ?? { hour: 9, minute: 0 },
    endDay: endDay.compare(start.day) < 0 ? start.day : endDay,
    endTime: end.time ?? { hour: 10, minute: 0 },
    tz: allDay ? fallbackTz : start.tz,
    endTz: !allDay && end.tz !== start.tz ? end.tz : null,
    location: fields.location ?? "",
    description: fields.description ?? "",
    url: fields.url ?? "",
    status: fields.status,
    transparency: fields.transparency,
    visibility: fields.visibility,
    conferences: fields.conferences.map((c) => ({ ...c })),
    attendees: fields.attendees.map((a) => ({ ...a })),
    alarms: fields.alarms.map((a) => ({ ...a })),
    recurrence: choiceOf(recurrence, seriesStart),
    scope,
  };
}

/** The fields of a form (keeping what the form does not show from `base`). */
export function fieldsOf(form: EventForm, base: EventFields): EventFields {
  const start: PimTime = form.allDay
    ? makePimTime(form.startDay, null, null)
    : makePimTime(form.startDay, form.startTime, form.tz);
  const end: PimTime = form.allDay
    ? makePimTime(form.endDay.add({ days: 1 }), null, null)
    : makePimTime(form.endDay, form.endTime, form.tz === null ? null : (form.endTz ?? form.tz));
  const text = (value: string) => (value.trim() === "" ? null : value);
  return {
    ...base,
    title: form.title.trim(),
    start,
    end,
    location: text(form.location),
    description: text(form.description),
    url: text(form.url),
    status: form.status,
    transparency: form.transparency,
    visibility: form.visibility,
    conferences: form.conferences.filter((c) => c.uri.trim() !== ""),
    attendees: form.attendees,
    alarms: form.alarms,
  };
}

/** Problems the editor finds before asking the core (which checks everything again). */
export function formIssue(form: EventForm): LocalizedMessage | null {
  const fields = fieldsOf(
    form,
    emptyEventFields(
      makePimTime(form.startDay, null, null),
      makePimTime(form.startDay, null, null),
    ),
  );
  const startMs = pimTimeIn(fields.start, "UTC").toDate().getTime();
  const endMs = pimTimeIn(fields.end, "UTC").toDate().getTime();
  if (endMs < startMs) return { id: "pim-issue-end-before-start" };
  if (form.attendees.some((a) => !/^[^\s@]+@[^\s@]+$/.test(a.email)))
    return { id: "pim-issue-invalid-email" };
  if (form.recurrence.type === "simple" && form.recurrence.rule.end.type === "until") {
    if (form.recurrence.rule.end.until.compare(form.startDay) < 0)
      return { id: "pim-issue-invalid-recurrence" };
  }
  return null;
}

/** The relative offset of an alarm as amount + unit (before / after). */
export function alarmParts(offsetSecs: number): {
  amount: number;
  unit: AlarmUnit;
  before: boolean;
} {
  const before = offsetSecs <= 0;
  const secs = Math.abs(offsetSecs);
  const unit =
    ([...ALARM_UNITS].reverse().find((u) => secs > 0 && secs % UNIT_SECONDS[u] === 0) as
      AlarmUnit | undefined) ?? "minutes";
  return { amount: Math.round(secs / UNIT_SECONDS[unit]), unit, before };
}

export function alarmOffset(amount: number, unit: AlarmUnit, before: boolean): number {
  const secs = Math.max(0, Math.trunc(amount)) * UNIT_SECONDS[unit];
  return before ? -secs : secs;
}

function zoneText(tz: string): string {
  return tz.replaceAll("_", " ");
}

/** The editor dialog. */
export function EventEditorDialog({ close }: DialogProps) {
  const t = useT();
  const target = usePim((s) => s.eventEditor);
  if (target === null) {
    return (
      <div data-part="pim.editor" data-state="empty">
        <p data-part="pim.editor.empty">{t("pim-editor-nothing")}</p>
        <button type="button" data-part="form.button" onClick={close}>
          {t("pim-close")}
        </button>
      </div>
    );
  }
  return <EventEditorLoader target={target} close={close} />;
}

function EventEditorLoader({ target, close }: { target: EventEditorTarget; close(): void }) {
  const t = useT();
  const event = useEvent(target.mode === "edit" ? target.eventId : null);
  const calendars = useCalendars();
  if (calendars.isPending || (target.mode === "edit" && event.isPending)) {
    return (
      <div data-part="pim.editor" data-state="loading" aria-busy="true">
        <p data-part="pim.editor.loading">{t("pim-loading")}</p>
      </div>
    );
  }
  if (event.isError || calendars.isError) {
    return (
      <div data-part="pim.editor" data-state="error">
        <Problem message={problemOf(event.error ?? calendars.error)} />
        <button type="button" data-part="form.button" onClick={close}>
          {t("pim-close")}
        </button>
      </div>
    );
  }
  return (
    <EventEditor
      key={
        target.mode === "edit" ? `${target.eventId}-${JSON.stringify(target.recurrenceId)}` : "new"
      }
      target={target}
      event={target.mode === "edit" ? (event.data ?? null) : null}
      calendars={calendars.data ?? []}
      close={close}
    />
  );
}

interface EditorProps {
  target: EventEditorTarget;
  event: CalendarEvent | null;
  calendars: readonly Calendar[];
  close(): void;
}

export function EventEditor({ target, event, calendars, close }: EditorProps) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useChangeContext();
  const ids = {
    title: useId(),
    calendar: useId(),
    tz: useId(),
    endTz: useId(),
    secondary: useId(),
    location: useId(),
    url: useId(),
    description: useId(),
    status: useId(),
    transparency: useId(),
    visibility: useId(),
    problem: useId(),
  };
  const writable = calendars.filter((c) => !c.read_only && c.components.includes("event"));
  const viewerTz = localZone();
  const myAddresses = useMyAddresses();
  const occurrence =
    target.mode === "edit" && target.recurrenceId !== null && event?.recurrence
      ? target.recurrenceId
      : null;
  const readOnly =
    event?.read_only === true ||
    (event !== null && calendars.find((c) => c.id === event.calendar_id)?.read_only === true);

  const initial = useMemo<EventForm>(() => {
    if (target.mode === "new") {
      const calendar =
        writable.find((c) => c.id === target.calendarId) ??
        writable.find((c) => c.source_id === "local") ??
        writable[0];
      const base = target.fields ?? emptyEventFields(target.start, target.end);
      const fields: EventFields = {
        ...base,
        alarms: target.fields ? base.alarms : (calendar?.settings.default_alarms ?? []),
      };
      return formOf(fields, calendar?.id ?? "", null, fields.start, viewerTz, "all");
    }
    const ev = event as CalendarEvent;
    if (occurrence !== null) {
      const override = ev.overrides.find((o) => sameTime(o.recurrence_id, occurrence));
      const fields = override?.fields ?? {
        ...ev.fields,
        start: target.start ?? ev.fields.start,
        end: target.end ?? ev.fields.end,
      };
      return formOf(fields, ev.calendar_id, ev.recurrence, ev.fields.start, viewerTz, "this");
    }
    return formOf(ev.fields, ev.calendar_id, ev.recurrence, ev.fields.start, viewerTz, "all");
    // The form is initialized once per editor (the component is keyed by its target).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const [form, setForm] = useState<EventForm>(initial);
  const [secondary, setSecondary] = useState<string>("");
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  const zones = useMemo(() => timeZoneNames(), []);
  const update = (patch: Partial<EventForm>) => setForm((f) => ({ ...f, ...patch }));

  const base = event
    ? event.fields
    : target.mode === "new"
      ? (target.fields ?? emptyEventFields(target.start, target.end))
      : null;
  const preview = fieldsOf(
    form,
    base ??
      emptyEventFields(
        target.mode === "new" ? target.start : { type: "date", date: "1970-01-01" },
        target.mode === "new" ? target.end : { type: "date", date: "1970-01-02" },
      ),
  );
  const scopeEditable = occurrence !== null;
  const recurrenceLocked = scopeEditable && form.scope === "this";

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    const issue = formIssue(form);
    if (issue) {
      setProblem(issue);
      return;
    }
    setBusy(true);
    setProblem(null);
    try {
      const fields = preview;
      if (target.mode === "new") {
        await saveEvent(
          ctx,
          { mode: "create", calendarId: form.calendarId },
          fields,
          recurrenceOf(form.recurrence, fields.start, null),
        );
      } else if (event) {
        if (form.calendarId !== event.calendar_id) {
          await ctx.client.request(
            { type: "MoveEvent", event_id: event.id, calendar_id: form.calendarId },
            "EventSaved",
          );
        }
        if (occurrence === null) {
          await saveEvent(
            ctx,
            { mode: "update", eventId: event.id, scope: "all", recurrenceId: null },
            fields,
            recurrenceOf(form.recurrence, fields.start, event.recurrence),
          );
        } else if (form.scope === "this") {
          await saveEvent(
            ctx,
            { mode: "update", eventId: event.id, scope: "this", recurrenceId: occurrence },
            fields,
            event.recurrence,
          );
        } else if (form.scope === "this-and-following") {
          await saveEvent(
            ctx,
            {
              mode: "update",
              eventId: event.id,
              scope: "this-and-following",
              recurrenceId: occurrence,
            },
            fields,
            recurrenceOf(form.recurrence, fields.start, event.recurrence),
          );
        } else {
          // All occurrences: move the series by what this occurrence moved.
          const shownStart =
            target.mode === "edit" && target.start ? target.start : event.fields.start;
          const shownEnd = target.mode === "edit" && target.end ? target.end : event.fields.end;
          const sameKind = shownStart.type === fields.start.type;
          const startDelta = wallDelta(shownStart, fields.start);
          const endDelta = wallDelta(shownEnd, fields.end);
          const seriesFields: EventFields = sameKind
            ? {
                ...fields,
                start: shiftPimTime(
                  withZone(event.fields.start, fields.start),
                  startDelta.days,
                  startDelta.minutes,
                ),
                end: shiftPimTime(
                  withZone(event.fields.end, fields.end),
                  endDelta.days,
                  endDelta.minutes,
                ),
              }
            : fields;
          await saveEvent(
            ctx,
            { mode: "update", eventId: event.id, scope: "all", recurrenceId: occurrence },
            seriesFields,
            recurrenceOf(form.recurrence, seriesFields.start, event.recurrence),
          );
        }
      }
      ctx.pim.getState().editEvent(null);
      close();
    } catch (error) {
      setProblem(problemOf(error));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!event) return;
    setBusy(true);
    setProblem(null);
    try {
      await deleteEvent(ctx, event.id, occurrence === null ? "all" : form.scope, occurrence);
      ctx.pim.getState().editEvent(null);
      close();
    } catch (error) {
      setProblem(problemOf(error));
    } finally {
      setBusy(false);
    }
  };

  const tzOptions = (
    <>
      {zones.map((zone) => (
        <option key={zone} value={zone}>
          {zoneText(zone)}
        </option>
      ))}
    </>
  );

  const secondaryLine = (time: PimTime) =>
    secondary && time.type !== "date"
      ? formatInstant(format, pimTimeIn(time, viewerTz).toDate().getTime(), secondary, {
          weekday: "short",
          hour: "numeric",
          minute: "2-digit",
          timeZoneName: "short",
        })
      : null;

  return (
    <form
      data-part="pim.editor"
      data-kind="event"
      onSubmit={(e) => void submit(e)}
      aria-busy={busy}
      aria-describedby={problem ? ids.problem : undefined}
      noValidate
    >
      {readOnly && <p data-part="pim.editor.read-only">{t("pim-editor-read-only")}</p>}
      <fieldset data-part="pim.editor.main" disabled={readOnly || busy}>
        <div data-part="form.field" data-field="title">
          <label data-part="form.label" htmlFor={ids.title}>
            {t("pim-event-title")}
          </label>
          <input
            id={ids.title}
            data-part="form.input"
            value={form.title}
            onChange={(e) => update({ title: e.target.value })}
            autoFocus
          />
        </div>
        <div data-part="form.field" data-field="calendar">
          <label data-part="form.label" htmlFor={ids.calendar}>
            {t("pim-event-calendar")}
          </label>
          <select
            id={ids.calendar}
            data-part="form.select"
            value={form.calendarId}
            onChange={(e) => update({ calendarId: e.target.value })}
          >
            {(readOnly ? calendars.filter((c) => c.id === form.calendarId) : writable).map((c) => (
              <option key={c.id} value={c.id}>
                {c.settings.name}
              </option>
            ))}
          </select>
        </div>

        {scopeEditable && (
          <fieldset data-part="pim.editor.scope">
            <legend data-part="form.section.title">{t("pim-scope-legend")}</legend>
            {EDIT_SCOPES.map((scope) => (
              <label key={scope} data-part="form.check" data-scope={scope}>
                <input
                  type="radio"
                  name={`${ids.title}-scope`}
                  value={scope}
                  checked={form.scope === scope}
                  onChange={() => update({ scope })}
                />
                <span>{t(`pim-scope-${scope}`)}</span>
              </label>
            ))}
          </fieldset>
        )}

        <fieldset data-part="pim.editor.when">
          <legend data-part="form.section.title">{t("pim-event-when")}</legend>
          <label data-part="form.check" data-field="all-day">
            <input
              type="checkbox"
              checked={form.allDay}
              onChange={(e) => update({ allDay: e.target.checked })}
            />
            <span>{t("pim-all-day")}</span>
          </label>
          <DateTimeInput
            label={t("pim-event-start")}
            field="start"
            day={form.startDay}
            time={form.allDay ? null : form.startTime}
            tz={form.tz ?? viewerTz}
            invalid={problem?.id === "pim-issue-end-before-start"}
            onChange={(day, time) => {
              // Moving the start keeps the length (end moves along).
              const deltaDays = day.compare(form.startDay);
              const minutes = time
                ? time.hour * 60 + time.minute - (form.startTime.hour * 60 + form.startTime.minute)
                : 0;
              const endTotal = form.endTime.hour * 60 + form.endTime.minute + minutes;
              const endDay = form.endDay.add({ days: deltaDays + Math.floor(endTotal / 1440) });
              const rest = ((endTotal % 1440) + 1440) % 1440;
              update({
                startDay: day,
                startTime: time ?? form.startTime,
                endDay,
                endTime: { hour: Math.floor(rest / 60), minute: rest % 60 },
              });
            }}
          />
          {secondaryLine(preview.start) && (
            <p data-part="pim.editor.secondary" data-field="start">
              {t("pim-secondary-time", {
                zone: zoneText(secondary),
                time: secondaryLine(preview.start) ?? "",
              })}
            </p>
          )}
          <DateTimeInput
            label={t("pim-event-end")}
            field="end"
            day={form.endDay}
            time={form.allDay ? null : form.endTime}
            tz={form.endTz ?? form.tz ?? viewerTz}
            invalid={problem?.id === "pim-issue-end-before-start"}
            describedBy={problem ? ids.problem : undefined}
            onChange={(day, time) => update({ endDay: day, endTime: time ?? form.endTime })}
          />
          {secondaryLine(preview.end) && (
            <p data-part="pim.editor.secondary" data-field="end">
              {t("pim-secondary-time", {
                zone: zoneText(secondary),
                time: secondaryLine(preview.end) ?? "",
              })}
            </p>
          )}
          {!form.allDay && (
            <>
              <div data-part="form.field" data-field="time-zone">
                <label data-part="form.label" htmlFor={ids.tz}>
                  {t("pim-event-time-zone")}
                </label>
                <select
                  id={ids.tz}
                  data-part="form.select"
                  value={form.tz ?? ""}
                  onChange={(e) =>
                    update({
                      tz: e.target.value === "" ? null : e.target.value,
                      endTz: e.target.value === "" ? null : form.endTz,
                    })
                  }
                >
                  <option value="">{t("pim-time-zone-floating")}</option>
                  {tzOptions}
                </select>
              </div>
              {form.tz !== null && (
                <label data-part="form.check" data-field="end-zone">
                  <input
                    type="checkbox"
                    checked={form.endTz !== null}
                    onChange={(e) => update({ endTz: e.target.checked ? form.tz : null })}
                  />
                  <span>{t("pim-event-end-zone-toggle")}</span>
                </label>
              )}
              {form.endTz !== null && (
                <div data-part="form.field" data-field="end-time-zone">
                  <label data-part="form.label" htmlFor={ids.endTz}>
                    {t("pim-event-end-time-zone")}
                  </label>
                  <select
                    id={ids.endTz}
                    data-part="form.select"
                    value={form.endTz}
                    onChange={(e) => update({ endTz: e.target.value })}
                  >
                    {tzOptions}
                  </select>
                </div>
              )}
              <div data-part="form.field" data-field="secondary-zone">
                <label data-part="form.label" htmlFor={ids.secondary}>
                  {t("pim-event-secondary-zone")}
                </label>
                <select
                  id={ids.secondary}
                  data-part="form.select"
                  value={secondary}
                  onChange={(e) => setSecondary(e.target.value)}
                >
                  <option value="">{t("pim-secondary-zone-none")}</option>
                  {tzOptions}
                </select>
              </div>
            </>
          )}
        </fieldset>

        <RecurrenceEditor
          value={form.recurrence}
          onChange={(recurrence) => update({ recurrence })}
          start={preview.start}
          disabled={recurrenceLocked}
          tz={form.tz ?? viewerTz}
        />
        {recurrenceLocked && <p data-part="pim.editor.hint">{t("pim-recurrence-locked")}</p>}

        <div data-part="form.field" data-field="location">
          <label data-part="form.label" htmlFor={ids.location}>
            {t("pim-event-location")}
          </label>
          <input
            id={ids.location}
            data-part="form.input"
            value={form.location}
            onChange={(e) => update({ location: e.target.value })}
          />
        </div>

        <ConferenceEditor
          value={form.conferences}
          onChange={(conferences) => update({ conferences })}
        />

        <div data-part="form.field" data-field="description">
          <label data-part="form.label" htmlFor={ids.description}>
            {t("pim-event-description")}
          </label>
          <textarea
            id={ids.description}
            data-part="form.textarea"
            value={form.description}
            onChange={(e) => update({ description: e.target.value })}
          />
        </div>
        <div data-part="form.field" data-field="url">
          <label data-part="form.label" htmlFor={ids.url}>
            {t("pim-event-url")}
          </label>
          <input
            id={ids.url}
            data-part="form.input"
            type="url"
            value={form.url}
            onChange={(e) => update({ url: e.target.value })}
          />
        </div>

        <AttendeePicker
          value={form.attendees}
          onChange={(attendees) => update({ attendees })}
          start={preview.start}
          end={preview.end}
          onTimeChange={(start, end) => {
            const s = splitPimTime(start);
            const e = splitPimTime(end);
            update({
              startDay: s.day,
              startTime: s.time ?? form.startTime,
              endDay: e.day,
              endTime: e.time ?? form.endTime,
            });
          }}
          organizer={event?.fields.organizer?.email ?? myAddresses[0] ?? null}
          eventId={event?.id ?? null}
          tz={viewerTz}
        />
        {target.mode === "edit" && event !== null && event.fields.attendees.length > 0 && (
          <MeetingPanel event={event} recurrenceId={occurrence} />
        )}
        <AlarmEditor value={form.alarms} onChange={(alarms) => update({ alarms })} tz={viewerTz} />

        <fieldset data-part="pim.editor.flags">
          <legend data-part="form.section.title">{t("pim-event-flags")}</legend>
          <div data-part="form.field" data-field="status">
            <label data-part="form.label" htmlFor={ids.status}>
              {t("pim-event-status")}
            </label>
            <select
              id={ids.status}
              data-part="form.select"
              value={form.status}
              onChange={(e) => update({ status: e.target.value as EventStatus })}
            >
              {EVENT_STATUSES.map((s) => (
                <option key={s} value={s}>
                  {t(`pim-status-${s}`)}
                </option>
              ))}
            </select>
          </div>
          <div data-part="form.field" data-field="transparency">
            <label data-part="form.label" htmlFor={ids.transparency}>
              {t("pim-event-transparency")}
            </label>
            <select
              id={ids.transparency}
              data-part="form.select"
              value={form.transparency}
              onChange={(e) => update({ transparency: e.target.value as Transparency })}
            >
              {TRANSPARENCIES.map((s) => (
                <option key={s} value={s}>
                  {t(`pim-transparency-${s}`)}
                </option>
              ))}
            </select>
          </div>
          <div data-part="form.field" data-field="visibility">
            <label data-part="form.label" htmlFor={ids.visibility}>
              {t("pim-event-visibility")}
            </label>
            <select
              id={ids.visibility}
              data-part="form.select"
              value={form.visibility}
              onChange={(e) => update({ visibility: e.target.value as Visibility })}
            >
              {VISIBILITIES.map((s) => (
                <option key={s} value={s}>
                  {t(`pim-visibility-${s}`)}
                </option>
              ))}
            </select>
          </div>
        </fieldset>
      </fieldset>

      <Problem message={problem} id={ids.problem} />
      <div data-part="pim.editor.actions">
        {!readOnly && (
          <button
            type="submit"
            data-part="form.button"
            data-variant="primary"
            disabled={busy || form.calendarId === ""}
          >
            {t("pim-save")}
          </button>
        )}
        {event && !readOnly && (
          <button
            type="button"
            data-part="form.button"
            data-variant="danger"
            disabled={busy}
            onClick={() => void remove()}
          >
            {occurrence !== null ? t(`pim-delete-scope-${form.scope}`) : t("pim-delete")}
          </button>
        )}
        <button
          type="button"
          data-part="form.button"
          onClick={() => {
            ctx.pim.getState().editEvent(null);
            close();
          }}
        >
          {t("pim-cancel")}
        </button>
      </div>
    </form>
  );
}

/** The series' time in the zone the user picked (the same wall-clock time). */
function withZone(series: PimTime, edited: PimTime): PimTime {
  if (series.type === "zoned" && edited.type === "zoned" && series.tz !== edited.tz) {
    return { type: "zoned", local: series.local, tz: edited.tz };
  }
  return series;
}

// ---------------------------------------------------------------------------
// Conferencing links

function ConferenceEditor({
  value,
  onChange,
}: {
  value: ConferenceLink[];
  onChange(value: ConferenceLink[]): void;
}) {
  const t = useT();
  const id = useId();
  const set = (index: number, patch: Partial<ConferenceLink>) =>
    onChange(value.map((c, i) => (i === index ? { ...c, ...patch } : c)));
  return (
    <fieldset data-part="pim.editor.conferences">
      <legend data-part="form.section.title">{t("pim-conference-legend")}</legend>
      {value.map((conference, index) => (
        <div key={index} data-part="pim.editor.row" data-row="conference">
          <div data-part="form.field" data-field="conference-uri">
            <label data-part="form.label" htmlFor={`${id}-uri-${index}`}>
              {t("pim-conference-uri")}
            </label>
            <input
              id={`${id}-uri-${index}`}
              data-part="form.input"
              type="url"
              value={conference.uri}
              onChange={(e) => set(index, { uri: e.target.value })}
            />
          </div>
          <div data-part="form.field" data-field="conference-label">
            <label data-part="form.label" htmlFor={`${id}-label-${index}`}>
              {t("pim-conference-label")}
            </label>
            <input
              id={`${id}-label-${index}`}
              data-part="form.input"
              value={conference.label ?? ""}
              onChange={(e) => set(index, { label: e.target.value === "" ? null : e.target.value })}
            />
          </div>
          {/^https:\/\//i.test(conference.uri) && (
            <a
              data-part="pim.editor.link"
              href={conference.uri}
              target="_blank"
              rel="noreferrer noopener"
            >
              {t("pim-conference-join")}
            </a>
          )}
          <button
            type="button"
            data-part="form.button"
            data-action="remove"
            aria-label={t("pim-conference-remove", { uri: conference.uri })}
            onClick={() => onChange(value.filter((_, i) => i !== index))}
          >
            {t("pim-remove")}
          </button>
        </div>
      ))}
      <button
        type="button"
        data-part="form.button"
        data-action="add-conference"
        onClick={() => onChange([...value, { uri: "", label: null, features: ["video"] }])}
      >
        {t("pim-conference-add")}
      </button>
    </fieldset>
  );
}

// ---------------------------------------------------------------------------
// Alarms

export function AlarmEditor({
  value,
  onChange,
  tz,
  related = "start",
}: {
  value: Alarm[];
  onChange(value: Alarm[]): void;
  tz: string;
  related?: "start" | "end";
}) {
  const t = useT();
  const format = useDateFormat();
  const id = useId();
  const set = (index: number, alarm: Alarm) =>
    onChange(value.map((a, i) => (i === index ? alarm : a)));
  return (
    <fieldset data-part="pim.editor.alarms">
      <legend data-part="form.section.title">{t("pim-alarms-legend")}</legend>
      {value.length === 0 && <p data-part="pim.editor.hint">{t("pim-alarms-none")}</p>}
      {value.map((alarm, index) => {
        if (alarm.trigger.type === "absolute") {
          return (
            <div key={index} data-part="pim.editor.row" data-row="alarm">
              <span data-part="pim.editor.alarm.absolute">
                {t("pim-alarm-at", {
                  time: formatInstant(format, alarm.trigger.at, tz, {
                    dateStyle: "medium",
                    timeStyle: "short",
                  }),
                })}
              </span>
              <button
                type="button"
                data-part="form.button"
                data-action="remove"
                aria-label={t("pim-alarm-remove", { index: index + 1 })}
                onClick={() => onChange(value.filter((_, i) => i !== index))}
              >
                {t("pim-remove")}
              </button>
            </div>
          );
        }
        const trigger = alarm.trigger;
        const parts = alarmParts(trigger.offset_secs);
        const setParts = (patch: Partial<typeof parts>) => {
          const next = { ...parts, ...patch };
          set(index, {
            ...alarm,
            trigger: { ...trigger, offset_secs: alarmOffset(next.amount, next.unit, next.before) },
          });
        };
        return (
          <div key={index} data-part="pim.editor.row" data-row="alarm">
            <div data-part="form.field" data-field="alarm-amount">
              <label data-part="form.label" htmlFor={`${id}-amount-${index}`}>
                {t("pim-alarm-amount", { index: index + 1 })}
              </label>
              <NumberField
                id={`${id}-amount-${index}`}
                value={parts.amount}
                min={0}
                max={9999}
                onChange={(amount) => setParts({ amount })}
              />
            </div>
            <div data-part="form.field" data-field="alarm-unit">
              <label data-part="form.label" htmlFor={`${id}-unit-${index}`}>
                {t("pim-alarm-unit")}
              </label>
              <select
                id={`${id}-unit-${index}`}
                data-part="form.select"
                value={parts.unit}
                onChange={(e) => setParts({ unit: e.target.value as AlarmUnit })}
              >
                {ALARM_UNITS.map((unit) => (
                  <option key={unit} value={unit}>
                    {t(`pim-alarm-unit-${unit}`, { count: parts.amount })}
                  </option>
                ))}
              </select>
            </div>
            <div data-part="form.field" data-field="alarm-when">
              <label data-part="form.label" htmlFor={`${id}-when-${index}`}>
                {t("pim-alarm-when")}
              </label>
              <select
                id={`${id}-when-${index}`}
                data-part="form.select"
                value={`${parts.before ? "before" : "after"}-${trigger.related}`}
                onChange={(e) => {
                  const [when, rel] = e.target.value.split("-") as [
                    "before" | "after",
                    "start" | "end",
                  ];
                  set(index, {
                    ...alarm,
                    trigger: {
                      type: "relative",
                      related: rel,
                      offset_secs: alarmOffset(parts.amount, parts.unit, when === "before"),
                    },
                  });
                }}
              >
                {["before-start", "after-start", "before-end", "after-end"].map((v) => (
                  <option key={v} value={v}>
                    {t(`pim-alarm-${v}`)}
                  </option>
                ))}
              </select>
            </div>
            <div data-part="form.field" data-field="alarm-action">
              <label data-part="form.label" htmlFor={`${id}-action-${index}`}>
                {t("pim-alarm-action")}
              </label>
              <select
                id={`${id}-action-${index}`}
                data-part="form.select"
                value={alarm.action}
                onChange={(e) => set(index, { ...alarm, action: e.target.value as AlarmAction })}
              >
                {ALARM_ACTIONS.map((action) => (
                  <option key={action} value={action}>
                    {t(`pim-alarm-action-${action}`)}
                  </option>
                ))}
              </select>
            </div>
            <button
              type="button"
              data-part="form.button"
              data-action="remove"
              aria-label={t("pim-alarm-remove", { index: index + 1 })}
              onClick={() => onChange(value.filter((_, i) => i !== index))}
            >
              {t("pim-remove")}
            </button>
          </div>
        );
      })}
      <button
        type="button"
        data-part="form.button"
        data-action="add-alarm"
        onClick={() =>
          onChange([
            ...value,
            {
              uid: null,
              action: "display",
              trigger: { type: "relative", offset_secs: -15 * 60, related },
              description: null,
              repeat: null,
              acknowledged: null,
            },
          ])
        }
      >
        {t("pim-alarm-add")}
      </button>
    </fieldset>
  );
}
