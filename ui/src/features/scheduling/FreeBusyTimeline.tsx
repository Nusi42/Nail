/**
 * The free/busy timeline of the attendee picker: one row per person with
 * their busy blocks (busy / tentative / out of office, from
 * `QueryFreeBusy`; where the data came from), the planned meeting drawn
 * over every row, who is busy then, and suggested free slots for everyone
 * (choosing one moves the meeting). Day navigation; times in the viewer's
 * zone and the UI locale.
 *
 * Positions are data, so they go through theme geometry (`useGeometry`),
 * never inline styles; `SCHEDULING_BASE_CSS` positions the blocks.
 *
 * R7 (S11, ADR 0246): wider lanes with each person's name and source, busy
 * blocks labelled only where availability is shared (none = hatched), the
 * chosen time as a frame that can be dragged or moved with the arrow keys,
 * suggestion chips that leave out the chosen time, and texts that never
 * contradict each other (people without shared availability are counted
 * apart, "no free time" only when the chosen time is not free either).
 */
import type { CalendarDate } from "@internationalized/date";
import {
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import type { AttendeeFreeBusy, EventId, IdentityId } from "../../api/generated";
import { errorMessageOf } from "../../api/client";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { useNow } from "../../state/clock";
import { useGeometry } from "../../theme/ThemeProvider";
import { Icon } from "../../widgets/Icon";
import { Problem } from "../pim/common";
import { formatDay, formatHour } from "../pim/fmt";
import { todayIn, toZoned, utcString } from "../pim/time";
import { clockRange } from "./fmt";
import { busyDuring, busySpans, ms, placeSpan, suggestSlots, type Span } from "./model";
import { useFreeBusy } from "./queries";

export interface Person {
  email: string;
  name: string | null;
}

export interface FreeBusyTimelineProps {
  /** Stable key for geometry ids (the widget's node key, or the editor's). */
  nodeKey: string;
  /** People to show (the organizer first, then the attendees). */
  people: readonly Person[];
  day: CalendarDate;
  onDay: (day: CalendarDate) => void;
  /** The planned meeting; `null` = none yet. */
  meeting: Span | null;
  /** Length of suggested slots in minutes. */
  durationMinutes: number;
  /** The viewer's zone. */
  tz: string;
  /** Visible working hours. */
  hourStart?: number | undefined;
  hourEnd?: number | undefined;
  identityId?: IdentityId | null;
  /** The event being edited (left out of the user's own busy time). */
  ignoreEventId?: EventId | null;
  maxSuggestions?: number;
  /** A suggested slot was chosen. */
  onPick?: (slot: Span) => void;
  /** "Now", for leaving out past slots (tests pass a fixed value). */
  now?: number | undefined;
}

function geom(...parts: (string | number)[]): string {
  return `sched-${parts.join("-").replace(/[^a-zA-Z0-9_-]/g, "_")}`;
}

function Block({
  id,
  part,
  left,
  width,
  kind,
  children,
}: {
  id: string;
  part: string;
  left: number;
  width: number;
  kind?: string;
  children?: ReactNode;
}) {
  useGeometry(id, {
    left: `${String(Math.round(left * 10000) / 100)}%`,
    width: `${String(Math.max(0.5, Math.round(width * 10000) / 100))}%`,
  });
  return (
    <span data-part={part} data-geom={id} data-kind={kind} aria-hidden="true">
      {children}
    </span>
  );
}

export function FreeBusyTimeline({
  nodeKey,
  people,
  day,
  onDay,
  meeting,
  durationMinutes,
  tz,
  hourStart = 8,
  hourEnd = 18,
  identityId = null,
  ignoreEventId = null,
  maxSuggestions = 5,
  onPick,
  now: fixedNow,
}: FreeBusyTimelineProps) {
  const t = useT();
  const clock = useNow();
  const now = fixedNow ?? clock;
  const format = useDateFormat();
  const start = Math.max(0, Math.min(23, hourStart));
  const end = Math.max(start + 1, Math.min(24, hourEnd));
  const dayStart = toZoned(day, tz).toDate().getTime();
  const dayEnd = toZoned(day.add({ days: 1 }), tz)
    .toDate()
    .getTime();
  const window: Span = {
    start: toZoned(day, tz).add({ hours: start }).toDate().getTime(),
    end: toZoned(day, tz).add({ hours: end }).toDate().getTime(),
  };
  const emails = people.map((p) => p.email);
  const query = useFreeBusy(
    emails,
    utcString(dayStart),
    utcString(dayEnd),
    identityId,
    ignoreEventId,
  );
  const rows: AttendeeFreeBusy[] = query.data?.attendees ?? [];
  const known = rows.filter((r) => r.error === null && r.source.type !== "unknown");
  const unknown = rows.length - known.length;
  const stepMs = (durationMinutes >= 30 ? 30 : 15) * 60_000;
  const earliest = Math.max(window.start, Math.ceil(now / stepMs) * stepMs);
  const busyNow = meeting ? busyDuring(rows, meeting) : [];
  const chosenFree = meeting !== null && busyNow.length === 0;
  // The chosen time is not suggested again (R7 S11: no "free" next to "no free time").
  const suggestions =
    query.data && durationMinutes > 0
      ? suggestSlots(
          busySpans(known),
          { start: earliest, end: window.end },
          durationMinutes * 60_000,
          stepMs,
          maxSuggestions + 1,
        )
          .filter((slot) => slot.start !== meeting?.start)
          .slice(0, maxSuggestions)
      : [];
  const nameOf = (email: string) => {
    const person = people.find((p) => p.email.toLowerCase() === email.toLowerCase());
    return person?.name ?? email;
  };
  const dayLabel = formatDay(format, day, { dateStyle: "full" });
  const hours = Array.from({ length: end - start }, (_, i) => start + i);
  const today = todayIn(tz, now);

  return (
    <section data-part="freebusy" aria-label={t("freebusy-label")} aria-busy={query.isFetching}>
      <div data-part="freebusy.nav">
        <button
          type="button"
          data-part="freebusy.previous"
          aria-label={t("freebusy-previous-day")}
          title={t("freebusy-previous-day")}
          onClick={() => {
            onDay(day.add({ days: -1 }));
          }}
        >
          <Icon name="chevron-left" part="freebusy.nav.icon" />
          <span data-part="freebusy.nav.text">{t("freebusy-previous-short")}</span>
        </button>
        <h3 data-part="freebusy.day">{dayLabel}</h3>
        <button
          type="button"
          data-part="freebusy.next"
          aria-label={t("freebusy-next-day")}
          title={t("freebusy-next-day")}
          onClick={() => {
            onDay(day.add({ days: 1 }));
          }}
        >
          <Icon name="chevron-right" part="freebusy.nav.icon" />
          <span data-part="freebusy.nav.text">{t("common-next-period")}</span>
        </button>
        {day.compare(today) !== 0 && (
          <button
            type="button"
            data-part="freebusy.today"
            onClick={() => {
              onDay(today);
            }}
          >
            {t("common-today")}
          </button>
        )}
      </div>

      {people.length === 0 ? (
        <p data-part="freebusy.empty">{t("freebusy-empty")}</p>
      ) : (
        <>
          <Lanes
            nodeKey={nodeKey}
            people={people}
            rows={rows}
            busyNow={busyNow}
            hours={hours}
            window={window}
            meeting={meeting}
            tz={tz}
            {...(onPick ? { onPick } : {})}
          />

          {meeting && query.data && (
            <p data-part="freebusy.verdict" data-state={chosenFree ? "free" : "busy"}>
              {chosenFree
                ? unknown > 0
                  ? t("pim-look-fb-free-chosen")
                  : t("freebusy-meeting-free")
                : t("freebusy-meeting-busy", {
                    count: busyNow.length,
                    names: busyNow.map(nameOf).join(", "),
                  })}
            </p>
          )}
          {query.data && unknown > 0 && (
            <p data-part="freebusy.unknown">{t("pim-look-fb-unknown", { count: unknown })}</p>
          )}

          {onPick && query.data && (suggestions.length > 0 || !chosenFree) && (
            <div data-part="freebusy.suggestions">
              <h4 data-part="freebusy.suggestions.title">
                {chosenFree ? t("pim-look-fb-other-times") : t("pim-look-fb-free-times")}
              </h4>
              {suggestions.length === 0 ? (
                <p data-part="freebusy.suggestions.none">
                  {t("pim-look-fb-none-day")}{" "}
                  <button
                    type="button"
                    data-part="form.button"
                    data-action="next-day"
                    onClick={() => {
                      onDay(day.add({ days: 1 }));
                    }}
                  >
                    {t("pim-look-fb-try-next")}
                  </button>
                </p>
              ) : (
                <ul data-part="freebusy.suggestion-list">
                  {suggestions.map((slot) => {
                    const text = clockRange(
                      t,
                      format,
                      utcString(slot.start),
                      utcString(slot.end),
                      tz,
                    );
                    return (
                      <li key={slot.start} data-part="freebusy.suggestion">
                        <button
                          type="button"
                          data-part="form.button"
                          aria-label={t("freebusy-suggestion-pick", { time: text, day: dayLabel })}
                          onClick={() => {
                            onPick(slot);
                          }}
                        >
                          {text}
                        </button>
                      </li>
                    );
                  })}
                </ul>
              )}
            </div>
          )}
          {onPick && meeting && <p data-part="freebusy.hint">{t("pim-look-fb-hint")}</p>}
        </>
      )}
      {query.isError && <Problem message={errorMessageOf(query.error)} part="freebusy.problem" />}
    </section>
  );
}

/** Snapping steps of the meeting frame (pointer and arrow keys). */
export const FRAME_STEP_MINUTES = 15;

/**
 * The lanes (R7 S11): an hour axis, one lane per person with their name and
 * where their availability comes from, busy blocks labelled with how busy
 * (only where availability is shared; "no information" is hatched), and
 * the chosen time as one frame across every lane. With `onPick` the frame
 * moves: drag it (15-minute steps) or focus it and use the arrow keys
 * (Shift: an hour).
 */
function Lanes({
  nodeKey,
  people,
  rows,
  busyNow,
  hours,
  window,
  meeting,
  tz,
  onPick,
}: {
  nodeKey: string;
  people: readonly Person[];
  rows: readonly AttendeeFreeBusy[];
  busyNow: readonly string[];
  hours: readonly number[];
  window: Span;
  meeting: Span | null;
  tz: string;
  onPick?: (slot: Span) => void;
}) {
  const t = useT();
  const format = useDateFormat();
  const lanesRef = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; width: number } | null>(null);
  const [offset, setOffset] = useState(0);
  const stepMs = FRAME_STEP_MINUTES * 60_000;
  const length = window.end - window.start;
  const shown = meeting ? { start: meeting.start + offset, end: meeting.end + offset } : null;
  const meetingPlace = shown ? placeSpan(shown, window) : null;
  const movable = onPick !== undefined && meeting !== null;

  const moveBy = (ms: number) => {
    if (!meeting || !onPick || ms === 0) return;
    onPick({ start: meeting.start + ms, end: meeting.end + ms });
  };
  const onPointerDown = (event: ReactPointerEvent<HTMLElement>) => {
    const track = lanesRef.current?.querySelector('[data-part="freebusy.track"]');
    const grabbed =
      event.target instanceof Element &&
      event.target.closest('[data-part="freebusy.frame"], [data-part="freebusy.meeting"]');
    if (!movable || !grabbed || !(track instanceof HTMLElement)) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { x: event.clientX, width: track.getBoundingClientRect().width };
  };
  const onPointerMove = (event: ReactPointerEvent<HTMLElement>) => {
    const d = drag.current;
    if (!d || d.width <= 0) return;
    const ms = ((event.clientX - d.x) / d.width) * length;
    setOffset(Math.round(ms / stepMs) * stepMs);
  };
  const onPointerUp = () => {
    if (!drag.current) return;
    drag.current = null;
    const moved = offset;
    setOffset(0);
    moveBy(moved);
  };
  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    const step = event.shiftKey ? 4 * stepMs : stepMs;
    if (event.key === "ArrowLeft" || event.key === "ArrowDown") {
      event.preventDefault();
      moveBy(-step);
    } else if (event.key === "ArrowRight" || event.key === "ArrowUp") {
      event.preventDefault();
      moveBy(step);
    }
  };
  const meetingText = shown
    ? clockRange(t, format, utcString(shown.start), utcString(shown.end), tz)
    : "";

  return (
    <div
      data-part="freebusy.lanes"
      data-state={movable ? "movable" : undefined}
      ref={lanesRef}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
    >
      <div data-part="freebusy.row" data-row="axis">
        <span data-part="freebusy.person" aria-hidden="true" />
        <div data-part="freebusy.scale">
          {hours.map((hour, i) => (
            <Block
              key={hour}
              id={geom(nodeKey, "hour", hour)}
              part="freebusy.hour"
              left={i / hours.length}
              width={1 / hours.length}
            >
              {formatHour(format, hour)}
            </Block>
          ))}
        </div>
      </div>
      {movable && meetingPlace && (
        <div data-part="freebusy.row" data-row="frame">
          <span data-part="freebusy.person" aria-hidden="true" />
          <div data-part="freebusy.slot">
            <Frame
              id={geom(nodeKey, "frame")}
              place={meetingPlace}
              label={t("pim-look-fb-slot", { time: meetingText })}
              value={Math.round(((shown?.start ?? window.start) - window.start) / 60_000)}
              max={Math.round(length / 60_000)}
              dragging={offset !== 0}
              onKeyDown={onKeyDown}
            />
          </div>
        </div>
      )}
      <ul data-part="freebusy.rows">
        {people.map((person, index) => {
          const row = rows.find((r) => r.email.toLowerCase() === person.email.toLowerCase());
          const name = person.name ?? person.email;
          const periods = row?.periods ?? [];
          const shared = row?.error === null && row.source.type !== "unknown";
          const summary = !row
            ? t("freebusy-row-loading", { name })
            : row.error !== null
              ? t("freebusy-row-error", { name })
              : row.source.type === "unknown"
                ? t("freebusy-row-unknown", { name })
                : periods.length === 0
                  ? t("freebusy-row-free", { name })
                  : t("freebusy-row-busy", {
                      name,
                      periods: periods
                        .map((p) =>
                          t("freebusy-period", {
                            time: clockRange(t, format, p.start, p.end, tz),
                            kind: t(`freebusy-kind-${p.kind}`),
                          }),
                        )
                        .join(", "),
                    });
          return (
            <li
              key={person.email}
              data-part="freebusy.row"
              data-source={row?.source.type}
              data-state={
                row?.error
                  ? "error"
                  : busyNow.includes(row?.email ?? "")
                    ? "busy"
                    : row && !shared
                      ? "unknown"
                      : undefined
              }
            >
              <span data-part="freebusy.person">{name}</span>
              {row && (
                <span data-part="freebusy.source">
                  {person.name
                    ? t("pim-look-fb-source", {
                        email: person.email,
                        source: t(`freebusy-source-${row.source.type}`),
                      })
                    : t(`freebusy-source-${row.source.type}`)}
                </span>
              )}
              <div data-part="freebusy.track" role="img" aria-label={summary}>
                {shared &&
                  periods.map((p) => {
                    const place = placeSpan({ start: ms(p.start), end: ms(p.end) }, window);
                    if (!place) return null;
                    return (
                      <Block
                        key={`${p.kind}-${p.start}`}
                        id={geom(nodeKey, "busy", index, p.kind, p.start)}
                        part="freebusy.busy"
                        kind={p.kind}
                        left={place.left}
                        width={place.width}
                      >
                        <span data-part="freebusy.busy.text">{t(`freebusy-kind-${p.kind}`)}</span>
                      </Block>
                    );
                  })}
                {row && !shared && row.error === null && (
                  <Block
                    id={geom(nodeKey, "unknown", index)}
                    part="freebusy.none"
                    left={0}
                    width={1}
                  >
                    <span data-part="freebusy.busy.text">{t("pim-look-fb-private")}</span>
                  </Block>
                )}
                {meetingPlace && (
                  <Block
                    id={geom(nodeKey, "meeting", index)}
                    part="freebusy.meeting"
                    left={meetingPlace.left}
                    width={meetingPlace.width}
                  />
                )}
              </div>
              {row?.error && (
                <span data-part="freebusy.error">
                  {t(
                    row.localized?.id ?? `error-${row.error.replaceAll("_", "-")}`,
                    row.localized?.args,
                  )}
                </span>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

/** The chosen time on the axis: a slider over the visible hours (minutes from their start). */
function Frame({
  id,
  place,
  label,
  value,
  max,
  dragging,
  onKeyDown,
}: {
  id: string;
  place: { left: number; width: number };
  label: string;
  value: number;
  max: number;
  dragging: boolean;
  onKeyDown: (event: KeyboardEvent<HTMLElement>) => void;
}) {
  useGeometry(id, {
    left: `${String(Math.round(place.left * 10000) / 100)}%`,
    width: `${String(Math.max(0.5, Math.round(place.width * 10000) / 100))}%`,
  });
  return (
    <span
      data-part="freebusy.frame"
      data-geom={id}
      data-state={dragging ? "dragging" : undefined}
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={max}
      aria-valuenow={value}
      aria-valuetext={label}
      onKeyDown={onKeyDown}
    />
  );
}
