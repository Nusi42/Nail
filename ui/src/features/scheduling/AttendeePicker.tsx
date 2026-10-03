/**
 * The attendee picker of the event editor (mount point `AttendeePicker`):
 * the attendees with their answers, roles and kinds (person, room,
 * resource), adding people with suggestions from the address books, and
 * the free/busy timeline of everyone at the meeting's day with suggested
 * free slots (choosing one moves the meeting).
 *
 * `FindTime` (widget `freebusy-timeline`, dialog `scheduling-find-time`)
 * uses the same picker on its own and then opens the PIM event editor with
 * the people and the chosen time.
 */
import { useEffect, useId, useState } from "react";
import type {
  Attendee,
  AttendeeKind,
  AttendeeRole,
  EventFields,
  EventId,
  IdentityId,
  PimTime,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { optionNumber, optionString, type WidgetProps } from "../../layout/widgetRegistry";
import { useIdentities } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import type { DialogProps } from "../../layout/dialogRegistry";
import {
  isKnownZone,
  localZone,
  pimTimeIn,
  sameKindAs,
  splitPimTime,
  todayIn,
  toZoned,
} from "../pim/time";
import { ATTENDEE_ROLES } from "../pim/constants";
import { emptyEventFields } from "../pim/defaults";
import { openNewEvent } from "../pim/newEvent";
import { useContactSearch } from "../pim/queries";
import { pimStoreFor } from "../pim/store";
import { FreeBusyTimeline } from "./FreeBusyTimeline";
import { isMine, plausibleEmail, type Span } from "./model";
import { useScheduling, useSchedulingStore } from "./store";
import { useMyAddresses } from "./queries";
import { fromAbsolute } from "@internationalized/date";

/** Kinds the picker offers (`CUTYPE`). */
export const PICKER_KINDS = [
  "individual",
  "room",
  "resource",
] as const satisfies readonly AttendeeKind[];

export interface AttendeePickerProps {
  value: Attendee[];
  onChange(value: Attendee[]): void;
  start: PimTime;
  end: PimTime;
  /** The meeting moved to a suggested slot (omit to hide suggestions). */
  onTimeChange?(start: PimTime, end: PimTime): void;
  /** The organizer's address, shown first in the timeline. */
  organizer?: string | null;
  /** The event being edited (its own time is not busy). */
  eventId?: EventId | null;
  identityId?: IdentityId | null;
  /** Stable key for geometry ids. */
  nodeKey?: string;
  hourStart?: number | undefined;
  hourEnd?: number | undefined;
  /** The viewer's zone (default: the system's). */
  tz?: string;
}

export function newAttendee(
  email: string,
  name: string | null,
  kind: AttendeeKind = "individual",
): Attendee {
  return {
    email,
    name,
    role: kind === "individual" ? "required" : "non-participant",
    status: "needs-action",
    kind,
    rsvp: kind === "individual",
    delegated_to: [],
    delegated_from: [],
  };
}

export function AttendeePicker({
  value,
  onChange,
  start,
  end,
  onTimeChange,
  organizer = null,
  eventId = null,
  identityId = null,
  nodeKey = "attendees",
  hourStart,
  hourEnd,
  tz = localZone(),
}: AttendeePickerProps) {
  const t = useT();
  const id = useId();
  const mine = useMyAddresses();
  const [email, setEmail] = useState("");
  const [kind, setKind] = useState<AttendeeKind>("individual");
  const [query, setQuery] = useState("");
  const [invalid, setInvalid] = useState(false);
  const startDay =
    start.type === "date"
      ? splitPimTime(start).day
      : todayIn(tz, pimTimeIn(start, tz).toDate().getTime());
  const [day, setDay] = useState(startDay);
  const [lastStart, setLastStart] = useState(startDay);
  if (lastStart.compare(startDay) !== 0) {
    setLastStart(startDay);
    setDay(startDay);
  }
  useEffect(() => {
    const timer = setTimeout(() => setQuery(email.trim().length >= 2 ? email.trim() : ""), 150);
    return () => clearTimeout(timer);
  }, [email]);
  const matches = useContactSearch(query, null);

  const add = (address: string, display: string | null) => {
    const clean = address.trim();
    if (!plausibleEmail(clean)) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    if (!value.some((a) => a.email.toLowerCase() === clean.toLowerCase())) {
      onChange([...value, newAttendee(clean, display?.trim() || null, kind)]);
    }
    setEmail("");
  };
  const suggestions = (matches.data ?? [])
    .flatMap((m) =>
      m.contact.emails.map((address) => ({
        address,
        name: m.contact.display_name,
        id: m.contact.id,
      })),
    )
    .filter((s) => !value.some((a) => a.email.toLowerCase() === s.address.toLowerCase()))
    .slice(0, 6);

  const allDay = start.type === "date";
  const meeting: Span | null = allDay
    ? null
    : {
        start: pimTimeIn(start, tz).toDate().getTime(),
        end: pimTimeIn(end, tz).toDate().getTime(),
      };
  const duration = meeting ? Math.max(15, Math.round((meeting.end - meeting.start) / 60_000)) : 60;
  const people = [
    ...(organizer && !value.some((a) => a.email.toLowerCase() === organizer.toLowerCase())
      ? [{ email: organizer, name: null }]
      : []),
    ...value.map((a) => ({ email: a.email, name: a.name })),
  ];

  return (
    <fieldset data-part="scheduling.attendees">
      <legend data-part="form.section.title">{t("scheduling-attendees-legend")}</legend>
      {value.length === 0 && <p data-part="scheduling.hint">{t("scheduling-attendees-none")}</p>}
      <ul data-part="scheduling.attendee-list">
        {value.map((attendee, index) => (
          <li
            key={attendee.email}
            data-part="scheduling.attendee"
            data-status={attendee.status}
            data-kind={attendee.kind}
          >
            <span data-part="scheduling.attendee.name">
              {attendee.name
                ? t("invitation-person", { name: attendee.name, email: attendee.email })
                : attendee.email}
              {isMine(attendee.email, mine) && (
                <span data-part="scheduling.attendee.me">{t("scheduling-attendee-me")}</span>
              )}
            </span>
            <span data-part="scheduling.attendee.status">
              {t(`pim-partstat-${attendee.status}`)}
            </span>
            <label data-part="form.label" htmlFor={`${id}-role-${index}`}>
              {t("scheduling-attendee-role", { email: attendee.email })}
            </label>
            <select
              id={`${id}-role-${index}`}
              data-part="form.select"
              value={attendee.role}
              onChange={(e) =>
                onChange(
                  value.map((a, i) =>
                    i === index ? { ...a, role: e.target.value as AttendeeRole } : a,
                  ),
                )
              }
            >
              {ATTENDEE_ROLES.map((role) => (
                <option key={role} value={role}>
                  {t(`pim-role-${role}`)}
                </option>
              ))}
            </select>
            <button
              type="button"
              data-part="form.button"
              data-action="remove"
              aria-label={t("scheduling-attendee-remove", { email: attendee.email })}
              onClick={() => onChange(value.filter((_, i) => i !== index))}
            >
              {t("scheduling-remove")}
            </button>
          </li>
        ))}
      </ul>
      <div data-part="scheduling.attendee-add">
        <div data-part="form.field" data-field="attendee-email">
          <label data-part="form.label" htmlFor={`${id}-email`}>
            {t("scheduling-attendee-email")}
          </label>
          <input
            id={`${id}-email`}
            data-part="form.input"
            type="email"
            autoComplete="off"
            value={email}
            aria-invalid={invalid}
            aria-describedby={invalid ? `${id}-invalid` : undefined}
            onChange={(e) => {
              setEmail(e.target.value);
              setInvalid(false);
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                add(email, null);
              }
            }}
          />
        </div>
        <div data-part="form.field" data-field="attendee-kind">
          <label data-part="form.label" htmlFor={`${id}-kind`}>
            {t("scheduling-attendee-kind")}
          </label>
          <select
            id={`${id}-kind`}
            data-part="form.select"
            value={kind}
            onChange={(e) => setKind(e.target.value as AttendeeKind)}
          >
            {PICKER_KINDS.map((k) => (
              <option key={k} value={k}>
                {t(`scheduling-kind-${k}`)}
              </option>
            ))}
          </select>
        </div>
        <button
          type="button"
          data-part="form.button"
          data-action="add-attendee"
          onClick={() => add(email, null)}
        >
          {t("scheduling-attendee-add")}
        </button>
      </div>
      {invalid && (
        <p id={`${id}-invalid`} data-part="scheduling.problem" role="alert">
          {t("scheduling-attendee-invalid")}
        </p>
      )}
      {suggestions.length > 0 && (
        <ul data-part="scheduling.suggestions" aria-label={t("scheduling-attendee-suggestions")}>
          {suggestions.map((s) => (
            <li key={`${s.id}-${s.address}`} data-part="scheduling.suggestion">
              <button type="button" data-part="form.button" onClick={() => add(s.address, s.name)}>
                {t("invitation-person", { name: s.name, email: s.address })}
              </button>
            </li>
          ))}
        </ul>
      )}
      <FreeBusyTimeline
        nodeKey={nodeKey}
        people={people}
        day={day}
        onDay={setDay}
        meeting={meeting}
        durationMinutes={duration}
        tz={tz}
        hourStart={hourStart}
        hourEnd={hourEnd}
        identityId={identityId}
        ignoreEventId={eventId}
        {...(onTimeChange && !allDay
          ? {
              onPick: (slot: Span) =>
                onTimeChange(
                  sameKindAs(start, fromAbsolute(slot.start, tz)),
                  sameKindAs(end, fromAbsolute(slot.end, tz)),
                ),
            }
          : {})}
      />
    </fieldset>
  );
}

/**
 * Where "find a time" starts: the next full hour today while a meeting of
 * `minutes` still fits into the working hours, else the next day's start.
 */
export function firstStart(
  tz: string,
  hourStart: number,
  hourEnd: number,
  minutes: number,
  now = Date.now(),
): PimTime {
  const like: PimTime = { type: "zoned", local: "2000-01-01T00:00:00", tz };
  const today = todayIn(tz, now);
  const next = fromAbsolute(now, tz)
    .add({ hours: 1 })
    .set({ minute: 0, second: 0, millisecond: 0 });
  const opens = toZoned(today, tz).add({ hours: hourStart });
  const closes = toZoned(today, tz).add({ hours: hourEnd });
  const candidate = next.compare(opens) < 0 ? opens : next;
  if (candidate.add({ minutes }).compare(closes) <= 0) return sameKindAs(like, candidate);
  return sameKindAs(like, toZoned(today.add({ days: 1 }), tz).add({ hours: hourStart }));
}

/** Lengths "find a time" offers, minutes. */
export const DURATIONS = [15, 30, 45, 60, 90, 120] as const;

/**
 * "Find a time": people, a length and a day; choosing a free slot opens
 * the PIM event editor with the people as attendees at that time (the user
 * as organizer), where saving and "send invitations" happen.
 */
export function FindTime({
  nodeKey,
  hourStart,
  hourEnd,
  defaultDuration = 30,
  tz = localZone(),
  onOpened,
}: {
  nodeKey: string;
  hourStart?: number | undefined;
  hourEnd?: number | undefined;
  defaultDuration?: number;
  tz?: string;
  onOpened?(): void;
}) {
  const t = useT();
  const id = useId();
  const ui = useUiStoreApi();
  const seed = useScheduling((s) => s.findTime);
  const scheduling = useSchedulingStore();
  const identities = useIdentities(null);
  const me = identities.data?.[0] ?? null;
  const [attendees, setAttendees] = useState<Attendee[]>(() =>
    (seed?.attendees ?? []).map((email) => newAttendee(email, null)),
  );
  const [duration, setDuration] = useState<number>(seed?.durationMinutes ?? defaultDuration);
  const [start, setStart] = useState<PimTime>(() =>
    firstStart(tz, hourStart ?? 8, hourEnd ?? 18, seed?.durationMinutes ?? defaultDuration),
  );
  const startMs = pimTimeIn(start, tz).toDate().getTime();
  const end = sameKindAs(start, fromAbsolute(startMs + duration * 60_000, tz));

  const create = () => {
    const fields: EventFields = {
      ...emptyEventFields(start, end),
      organizer: me ? { email: me.email, name: me.name } : null,
      attendees,
    };
    const split = splitPimTime(start);
    openNewEvent({ store: ui, pim: pimStoreFor(ui) }, split.day, split.time, tz, null, fields);
    scheduling.getState().setFindTime(null);
    onOpened?.();
  };

  return (
    <div data-part="scheduling.find-time">
      <div data-part="form.field" data-field="duration">
        <label data-part="form.label" htmlFor={`${id}-duration`}>
          {t("scheduling-find-duration")}
        </label>
        <select
          id={`${id}-duration`}
          data-part="form.select"
          value={duration}
          onChange={(e) => setDuration(Number(e.target.value))}
        >
          {DURATIONS.map((m) => (
            <option key={m} value={m}>
              {t("scheduling-minutes", { minutes: m })}
            </option>
          ))}
        </select>
      </div>
      <AttendeePicker
        value={attendees}
        onChange={setAttendees}
        start={start}
        end={end}
        onTimeChange={(s) => setStart(s)}
        organizer={me?.email ?? null}
        nodeKey={nodeKey}
        hourStart={hourStart}
        hourEnd={hourEnd}
        tz={tz}
      />
      <div data-part="scheduling.find-time.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          disabled={attendees.length === 0}
          onClick={create}
        >
          {t("scheduling-find-create")}
        </button>
      </div>
    </div>
  );
}

/** Layout widget `freebusy-timeline`: "find a time" in a layout window. */
export function FreeBusyWidget({ node, nodeKey }: WidgetProps) {
  const zone = optionString(node, "time-zone", "");
  return (
    <section data-part="freebusy-timeline" data-node-id={node.id ?? undefined}>
      <FindTime
        nodeKey={nodeKey}
        hourStart={optionNumber(node, "hour-start", 8)}
        hourEnd={optionNumber(node, "hour-end", 18)}
        defaultDuration={optionNumber(node, "duration", 30)}
        tz={zone && isKnownZone(zone) ? zone : localZone()}
      />
    </section>
  );
}

/** Dialog `scheduling-find-time`. */
export function FindTimeDialog({ close }: DialogProps) {
  const t = useT();
  return (
    <div data-part="scheduling.dialog" data-dialog="find-time">
      <FindTime nodeKey="find-time-dialog" onOpened={close} />
      <div data-part="scheduling.dialog.actions">
        <button type="button" data-part="form.button" onClick={close}>
          {t("scheduling-close")}
        </button>
      </div>
    </div>
  );
}
