/**
 * The quick look (R7 M4 / S1, ADR 0246): a popover next to the event a
 * click opened (`PimState.quickLook`), answering when, where, who and how
 * to join without leaving the view.
 *
 * Content: the title with the calendar's colour dot; the day, the time
 * range with its length and zone; the calendar; the place; the people with
 * their answers (a summary line and the first few names); the call link
 * (Join). Actions: Edit (the full editor), Delete (the calendar's delete:
 * a recurring occurrence asks "this / following / all" first, a single
 * event goes through the queue and its undo), Send update for an
 * organizer, and Accept / Maybe / Decline for an invitation — an
 * unanswered one (`needs-action`) also lists what it overlaps.
 *
 * Keyboard and focus: the popover is a dialog named by the title; Edit has
 * the focus on open (Enter edits), Delete / Backspace delete, Esc or a
 * press outside close it and the focus goes back to the card (the shared
 * `Popover`). The host is mounted once (the feature's `effects`).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState, type KeyboardEvent, type ReactNode } from "react";
import type {
  CalendarEvent,
  EventFields,
  EventInstance,
  InvitationResponse,
  LocalizedMessage,
} from "../../api/generated";
import { errorMessageOf } from "../../api/client";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { Popover } from "../../widgets/Popover";
import { nonEmpty } from "../../text/empty";
import { isTextInput } from "../menus/targets";
import { AnswerForm } from "../scheduling/Answer";
import { isMine, RESPONSES } from "../scheduling/model";
import { scheduleWrite, useMyAddresses, useSchedulingSettings } from "../scheduling/queries";
import { requestDelete } from "./changes";
import { colorOf, geomId, Problem, Swatch, useChangeContext } from "./common";
import { useOpenInstance } from "./EventItem";
import {
  joinLinkOf,
  lengthText,
  minutesOf,
  myAttendee,
  overlapsOf,
  peopleSummary,
} from "./eventLook";
import { formatClock, formatDay, zoneLabel } from "./fmt";
import { useCalendars, useEvent, useInstances } from "./queries";
import { usePim, usePimStore } from "./store";
import { instantMs, localZone, pimTimeIn, sameTime, splitPimTime, toCalendarDate } from "./time";

/** People the quick look names before "and n more". */
export const QUICK_LOOK_PEOPLE = 4;

/** Shows the open quick look, if any. */
export function EventPopoverHost() {
  const t = useT();
  const quickLook = usePim((s) => s.quickLook);
  const pim = usePimStore();
  if (quickLook === null) return null;
  const { instance, anchor } = quickLook;
  const close = () => {
    pim.getState().setQuickLook(null);
  };
  return (
    <Popover
      anchor={anchor}
      label={nonEmpty(instance.title) ?? t("common-no-title")}
      part="pim.popover"
      onClose={close}
    >
      <QuickLook
        key={`${instance.event_id}-${JSON.stringify(instance.recurrence_id)}`}
        instance={instance}
        close={close}
      />
    </Popover>
  );
}

/** The fields the occurrence shows: its override's, else the series'. */
export function occurrenceFields(event: CalendarEvent, instance: EventInstance): EventFields {
  const rid = instance.recurrence_id;
  if (rid === null) return event.fields;
  return event.overrides.find((o) => sameTime(o.recurrence_id, rid))?.fields ?? event.fields;
}

/** Who a person is in the quick look's list (`pim-look-person`'s selector). */
function roleOf(organizer: boolean, you: boolean): string {
  if (organizer) return you ? "organizer-you" : "organizer";
  return you ? "you" : "other";
}

function QuickLook({ instance, close }: { instance: EventInstance; close: () => void }) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useChangeContext();
  const client = useClient();
  const queryClient = useQueryClient();
  const openEditor = useOpenInstance();
  const event = useEvent(instance.event_id);
  const calendars = useCalendars();
  const settings = useSchedulingSettings();
  const mine = useMyAddresses();
  const tz = localZone();
  const overlapping = useInstances({ from: instance.start_utc, to: instance.end_utc }, null, tz);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [done, setDone] = useState<LocalizedMessage | null>(null);

  const calendar = calendars.data?.find((c) => c.id === instance.calendar_id) ?? null;
  const title = nonEmpty(instance.title) ?? t("common-no-title");
  const fields = event.data ? occurrenceFields(event.data, instance) : null;
  const me = fields ? myAttendee(fields.attendees, mine) : null;
  const organizer = fields?.organizer ?? null;
  const organizing = organizer !== null && isMine(organizer.email, mine);
  // The event's own attendee entry is the truth; the instance's until it loaded.
  const myStatus = organizing ? null : (me?.status ?? instance.my_status);
  const readOnly = instance.read_only || calendar?.read_only === true;
  const color = colorOf(calendars.data, instance.calendar_id, instance.color);

  const run = async (write: () => Promise<unknown>, success: LocalizedMessage) => {
    setBusy(true);
    setProblem(null);
    setDone(null);
    try {
      await write();
      setDone(success);
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };

  const edit = () => {
    close();
    openEditor(instance);
  };
  const remove = () => {
    if (readOnly) return;
    close();
    void requestDelete(ctx, instance).catch((error: unknown) => {
      ctx.pim.getState().pushToast({
        kind: "problem",
        id: `delete-${instance.event_id}-${String(Date.now())}`,
        message: errorMessageOf(error),
      });
    });
  };
  const answer = (response: InvitationResponse, comment: string | null, send: boolean) =>
    void run(
      () =>
        scheduleWrite(
          { client, queryClient },
          {
            type: "RespondToInvitation",
            target: { type: "event", event_id: instance.event_id, recurrence_id: null },
            response,
            comment,
            send,
            calendar_id: null,
          },
        ),
      { id: send ? `invitation-done-${response}-sent` : `invitation-done-${response}` },
    );
  const sendUpdate = () =>
    void run(
      () =>
        scheduleWrite(
          { client, queryClient },
          {
            type: "SendInvitations",
            event_id: instance.event_id,
            recurrence_id: instance.overridden ? instance.recurrence_id : null,
            attendees: [],
            comment: null,
          },
        ),
      { id: "pim-look-update-sent" },
    );

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    // A field that takes text keeps its own Delete and Enter.
    if (e.target instanceof Element && isTextInput(e.target)) return;
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      remove();
    } else if (e.key === "Enter" && e.target === e.currentTarget) {
      e.preventDefault();
      edit();
    }
  };

  const conflicts = myStatus === "needs-action" ? overlapsOf(instance, overlapping.data ?? []) : [];
  const attendees = fields?.attendees ?? [];
  const organizerListed =
    organizer !== null &&
    attendees.some((a) => a.email.toLowerCase() === organizer.email.toLowerCase());
  const people = [
    ...(organizer && !organizerListed
      ? [{ email: organizer.email, name: organizer.name, status: null }]
      : []),
    ...attendees.map((a) => ({ email: a.email, name: a.name, status: a.status })),
  ];
  const join = fields ? joinLinkOf(fields.conferences) : null;
  const location = fields?.location ?? instance.location;

  return (
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions -- the quick look's keys (Delete, Enter) work from any of its controls; the popover itself is the dialog.
    <div data-part="pim.popover.body" data-status={myStatus ?? undefined} onKeyDown={onKeyDown}>
      <header data-part="pim.popover.header">
        <Swatch
          geom={geomId("look", instance.calendar_id, color)}
          color={color}
          part="pim.popover.dot"
        />
        <h2 data-part="pim.popover.title">{title}</h2>
      </header>
      <dl data-part="pim.popover.facts">
        <Fact field="when" label={t("common-when")}>
          <WhenText instance={instance} tz={tz} />
        </Fact>
        {calendar && (
          <Fact field="calendar" label={t("pim-look-calendar")}>
            {calendar.settings.name}
            {readOnly && <span data-part="pim.popover.read-only">{t("pim-look-read-only")}</span>}
          </Fact>
        )}
        {location && (
          <Fact field="place" label={t("pim-look-place")}>
            {location}
          </Fact>
        )}
        {people.length > 0 && (
          <Fact field="people" label={t("pim-look-people")}>
            <span data-part="pim.popover.summary">
              {peopleSummary(
                t,
                format.locale,
                attendees.map((a) => a.status),
              )}
            </span>
            <ul data-part="pim.popover.people">
              {people.slice(0, QUICK_LOOK_PEOPLE).map((p) => (
                <li
                  key={p.email}
                  data-part="pim.popover.person"
                  data-status={p.status ?? undefined}
                >
                  <span data-part="pim.popover.person.name">
                    {t("pim-look-person", {
                      name: nonEmpty(p.name) ?? p.email,
                      role: roleOf(
                        organizer?.email.toLowerCase() === p.email.toLowerCase(),
                        isMine(p.email, mine),
                      ),
                    })}
                  </span>
                  {p.status && (
                    <span data-part="pim.popover.person.status">
                      {t(`pim-look-status-${p.status}`)}
                    </span>
                  )}
                </li>
              ))}
              {people.length > QUICK_LOOK_PEOPLE && (
                <li data-part="pim.popover.person" data-state="more">
                  {t("pim-look-more-people", { count: people.length - QUICK_LOOK_PEOPLE })}
                </li>
              )}
            </ul>
          </Fact>
        )}
      </dl>
      {event.isPending && <p data-part="pim.popover.loading">{t("pim-look-loading")}</p>}
      {conflicts.length > 0 && (
        <p data-part="pim.popover.conflicts">
          {t("pim-look-overlaps", {
            count: conflicts.length,
            titles: conflicts.map((c) => nonEmpty(c.title) ?? t("common-no-title")).join(", "),
          })}
        </p>
      )}
      {me !== null && !organizing && !readOnly && (
        <AnswerForm
          current={myStatus}
          available={RESPONSES}
          organizer={organizer ? (nonEmpty(organizer.name) ?? organizer.email) : null}
          confirm={settings.data?.confirm_replies ?? false}
          busy={busy}
          onAnswer={answer}
          compact
        />
      )}
      <div data-part="pim.popover.actions">
        <button
          type="button"
          data-part="form.button"
          data-action="edit"
          data-variant="primary"
          data-autofocus
          onClick={edit}
        >
          {readOnly ? t("common-open") : t("common-edit")}
        </button>
        {join && (
          <a
            data-part="form.button"
            data-action="join"
            data-variant="ghost"
            href={join}
            target="_blank"
            rel="noreferrer noopener"
          >
            {t("pim-look-join")}
          </a>
        )}
        {organizing && attendees.length > 0 && !readOnly && (
          <button
            type="button"
            data-part="form.button"
            data-action="send-update"
            disabled={busy}
            onClick={sendUpdate}
          >
            {t("pim-look-send-update")}
          </button>
        )}
        {!readOnly && (
          <button
            type="button"
            data-part="form.button"
            data-action="delete"
            data-variant="danger"
            onClick={remove}
          >
            {t("common-delete")}
          </button>
        )}
      </div>
      <Problem message={problem} part="pim.popover.problem" />
      <p data-part="pim.popover.done" role="status">
        {done ? t(done.id, done.args) : ""}
      </p>
      <p data-part="pim.popover.keys">
        <KeyHint keyId="pim-look-key-enter" actionId="pim-look-key-edit" />
        {!readOnly && <KeyHint keyId="pim-look-key-delete" actionId="pim-look-key-delete-action" />}
        <KeyHint keyId="pim-look-key-escape" actionId="pim-look-key-close" />
      </p>
    </div>
  );
}

/** "Friday, November 27" and "3:00 – 4:00 PM · 1 hour · UTC" (or the all-day days). */
function WhenText({ instance, tz }: { instance: EventInstance; tz: string }) {
  const t = useT();
  const format = useDateFormat();
  const full = (day: ReturnType<typeof toCalendarDate>) =>
    formatDay(format, day, { dateStyle: "full" });
  if (instance.all_day) {
    const first = splitPimTime(instance.start).day;
    const last =
      instance.end.type === "date" ? splitPimTime(instance.end).day.add({ days: -1 }) : first;
    return (
      <>
        <span data-part="pim.popover.day">
          {last.compare(first) > 0
            ? t("common-time-range-all-day", { start: full(first), end: full(last) })
            : full(first)}
        </span>
        <span data-part="pim.popover.time">{t("common-all-day")}</span>
      </>
    );
  }
  const range = t("common-time-range", {
    start: formatClock(format, instance.start_utc, tz),
    end: formatClock(format, instance.end_utc, tz),
  });
  return (
    <>
      <span data-part="pim.popover.day">{full(toCalendarDate(pimTimeIn(instance.start, tz)))}</span>
      <span data-part="pim.popover.time">
        {t("pim-look-when-zone", {
          range,
          length: lengthText(t, format.locale, minutesOf(instance.start_utc, instance.end_utc)),
          zone: zoneLabel(format, instantMs(instance.start_utc), tz),
        })}
      </span>
    </>
  );
}

/** One key and what it does ("Enter edit"); the theme separates the hints. */
function KeyHint({ keyId, actionId }: { keyId: string; actionId: string }) {
  const t = useT();
  return (
    <span data-part="pim.popover.hint">
      <kbd data-part="pim.popover.key">{t(keyId)}</kbd>
      <span data-part="pim.popover.hint.text">{t(actionId)}</span>
    </span>
  );
}

function Fact({ field, label, children }: { field: string; label: string; children: ReactNode }) {
  return (
    <div data-part="pim.popover.fact" data-field={field}>
      <dt data-part="pim.popover.fact.label">{label}</dt>
      <dd data-part="pim.popover.fact.value">{children}</dd>
    </div>
  );
}
