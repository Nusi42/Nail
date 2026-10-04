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
 */
import type { CalendarDate } from "@internationalized/date";
import type { ReactNode } from "react";
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
  const known = rows.filter((r) => r.error === null);
  const stepMs = (durationMinutes >= 30 ? 30 : 15) * 60_000;
  const earliest = Math.max(window.start, Math.ceil(now / stepMs) * stepMs);
  const suggestions =
    query.data && durationMinutes > 0
      ? suggestSlots(
          busySpans(known),
          { start: earliest, end: window.end },
          durationMinutes * 60_000,
          stepMs,
          maxSuggestions,
        )
      : [];
  const busyNow = meeting ? busyDuring(rows, meeting) : [];
  const nameOf = (email: string) => {
    const person = people.find((p) => p.email.toLowerCase() === email.toLowerCase());
    return person?.name ?? email;
  };
  const meetingPlace = meeting ? placeSpan(meeting, window) : null;
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
          <span data-part="freebusy.nav.text">{t("freebusy-next-short")}</span>
        </button>
        {day.compare(today) !== 0 && (
          <button
            type="button"
            data-part="freebusy.today"
            onClick={() => {
              onDay(today);
            }}
          >
            {t("freebusy-today")}
          </button>
        )}
      </div>

      {people.length === 0 ? (
        <p data-part="freebusy.empty">{t("freebusy-empty")}</p>
      ) : (
        <>
          <div data-part="freebusy.scale" aria-hidden="true">
            {hours.map((hour) => (
              <Block
                key={hour}
                id={geom(nodeKey, "hour", hour)}
                part="freebusy.hour"
                left={(hour - start) / (end - start)}
                width={1 / (end - start)}
              >
                {formatHour(format, hour)}
              </Block>
            ))}
          </div>
          <ul data-part="freebusy.rows">
            {people.map((person, index) => {
              const row = rows.find((r) => r.email.toLowerCase() === person.email.toLowerCase());
              const name = person.name ?? person.email;
              const periods = row?.periods ?? [];
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
                    row?.error ? "error" : busyNow.includes(row?.email ?? "") ? "busy" : undefined
                  }
                >
                  <span data-part="freebusy.person">{name}</span>
                  {row && (
                    <span data-part="freebusy.source">
                      {t(`freebusy-source-${row.source.type}`)}
                    </span>
                  )}
                  <div data-part="freebusy.track" role="img" aria-label={summary}>
                    {periods.map((p) => {
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
                        />
                      );
                    })}
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

          {meeting && query.data && (
            <p data-part="freebusy.verdict" data-state={busyNow.length > 0 ? "busy" : "free"}>
              {busyNow.length > 0
                ? t("freebusy-meeting-busy", {
                    count: busyNow.length,
                    names: busyNow.map(nameOf).join(", "),
                  })
                : t("freebusy-meeting-free")}
            </p>
          )}

          {onPick && query.data && (
            <div data-part="freebusy.suggestions">
              <h4 data-part="freebusy.suggestions.title">{t("freebusy-suggestions")}</h4>
              {suggestions.length === 0 ? (
                <p data-part="freebusy.suggestions.none">{t("freebusy-suggestions-none")}</p>
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
        </>
      )}
      {query.isError && <Problem message={errorMessageOf(query.error)} part="freebusy.problem" />}
    </section>
  );
}
