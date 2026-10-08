/**
 * Pure helpers of the scheduling UI: busy periods (merging like
 * `BusyPeriod::merge` in `nail-api`), suggested free slots for the
 * attendee picker, the answer an invitation action stands for, and small
 * address helpers. No React, no requests: unit-tested in `model.test.ts`.
 *
 * Suggesting free slots is presentation over the core's free/busy answer
 * (`QueryFreeBusy`): the core decides who is busy when; the UI only looks
 * for gaps of the meeting's length inside the visible working hours.
 */
import type {
  AttendeeFreeBusy,
  BusyKind,
  BusyPeriod,
  Invitation,
  InvitationAction,
  InvitationResponse,
  ParticipationStatus,
} from "../../api/generated";

/** The answers in the order the card offers them. */
export const RESPONSES: readonly InvitationResponse[] = ["accept", "tentative", "decline"];

/** `BusyKind` order of strength (the stronger one is drawn on top). */
export const BUSY_KINDS: readonly BusyKind[] = ["tentative", "busy", "unavailable"];

/** The participation status an answer sets. */
export function statusOfResponse(response: InvitationResponse): ParticipationStatus {
  switch (response) {
    case "accept":
      return "accepted";
    case "tentative":
      return "tentative";
    case "decline":
      return "declined";
  }
}

/** The answer a participation status stands for, `null` when it is none. */
export function responseOfStatus(status: ParticipationStatus | null): InvitationResponse | null {
  switch (status) {
    case "accepted":
      return "accept";
    case "tentative":
      return "tentative";
    case "declined":
      return "decline";
    default:
      return null;
  }
}

/** Whether the card may answer (accept / tentative / decline) at all. */
export function canRespond(invitation: Pick<Invitation, "actions">): boolean {
  return RESPONSES.some((r) => invitation.actions.includes(r));
}

/** The actions that are neither answers nor "propose a new time" (shown as buttons). */
export const OTHER_ACTIONS: readonly InvitationAction[] = [
  "add-to-calendar",
  "update-calendar",
  "remove-from-calendar",
  "apply-reply",
  "accept-counter",
  "decline-counter",
  "send-current",
  "refresh",
  "forward",
  "open-event",
];

/** Milliseconds of an RFC 3339 instant (`NaN` when malformed). */
export function ms(utc: string): number {
  return Date.parse(utc);
}

/**
 * Merges overlapping and touching periods of the same kind and sorts by
 * start, like `BusyPeriod::merge` (periods with `end <= start` are dropped;
 * where kinds overlap, each keeps its own period).
 */
export function mergeBusy(periods: readonly BusyPeriod[]): BusyPeriod[] {
  const valid = periods
    .filter((p) => ms(p.end) > ms(p.start))
    .map((p) => ({ ...p }))
    .sort(
      (a, b) =>
        BUSY_KINDS.indexOf(a.kind) - BUSY_KINDS.indexOf(b.kind) ||
        ms(a.start) - ms(b.start) ||
        ms(a.end) - ms(b.end),
    );
  const out: BusyPeriod[] = [];
  for (const period of valid) {
    const last = out[out.length - 1];
    if (last?.kind === period.kind && ms(period.start) <= ms(last.end)) {
      if (ms(period.end) > ms(last.end)) last.end = period.end;
    } else {
      out.push(period);
    }
  }
  return out.sort(
    (a, b) => ms(a.start) - ms(b.start) || BUSY_KINDS.indexOf(a.kind) - BUSY_KINDS.indexOf(b.kind),
  );
}

/** A stretch of time in milliseconds. */
export interface Span {
  start: number;
  end: number;
}

/** Whether two spans overlap (touching is not overlapping). */
export function overlaps(a: Span, b: Span): boolean {
  return a.start < b.end && b.start < a.end;
}

/** Every busy period of every attendee that has data, as spans. */
export function busySpans(attendees: readonly AttendeeFreeBusy[]): Span[] {
  return attendees.flatMap((a) => a.periods.map((p) => ({ start: ms(p.start), end: ms(p.end) })));
}

/** The attendees busy during `span` (their addresses, in the given order). */
export function busyDuring(attendees: readonly AttendeeFreeBusy[], span: Span): string[] {
  return attendees
    .filter((a) => a.periods.some((p) => overlaps({ start: ms(p.start), end: ms(p.end) }, span)))
    .map((a) => a.email);
}

/**
 * Free slots of `durationMs` inside `window` that overlap no busy span,
 * starting on multiples of `stepMs` (from the window's start), earliest
 * first, at most `max`. Slots never overlap each other.
 */
export function suggestSlots(
  busy: readonly Span[],
  window: Span,
  durationMs: number,
  stepMs: number,
  max: number,
): Span[] {
  if (durationMs <= 0 || stepMs <= 0 || max <= 0) return [];
  const sorted = [...busy].sort((a, b) => a.start - b.start);
  const out: Span[] = [];
  let start = window.start;
  while (start + durationMs <= window.end && out.length < max) {
    const slot = { start, end: start + durationMs };
    const blocker = sorted.find((b) => overlaps(b, slot));
    if (!blocker) {
      out.push(slot);
      start = slot.end;
    } else {
      // Jump to the next step at or after the blocking period's end.
      const steps = Math.ceil((blocker.end - window.start) / stepMs);
      start = Math.max(start + stepMs, window.start + steps * stepMs);
    }
  }
  return out;
}

/** Left offset and width (fractions of the window) of a span, clipped; `null` when outside. */
export function placeSpan(span: Span, window: Span): { left: number; width: number } | null {
  const length = window.end - window.start;
  if (length <= 0) return null;
  const left = Math.max(0, (span.start - window.start) / length);
  const right = Math.min(1, (span.end - window.start) / length);
  if (right <= 0 || left >= 1 || right <= left) return null;
  return { left, width: right - left };
}

/** Addresses typed into one field, separated by commas, semicolons or spaces. */
export function splitAddresses(text: string): string[] {
  return text
    .split(/[,;\s]+/)
    .map((a) => a.trim())
    .filter(Boolean);
}

/** Whether `email` is one of `mine` (case-insensitive). */
export function isMine(email: string | null | undefined, mine: readonly string[]): boolean {
  if (!email) return false;
  const lower = email.toLowerCase();
  return mine.some((m) => m.toLowerCase() === lower);
}

/** Counts of answers among attendees, for the card's summary line. */
export function answerCounts(
  statuses: readonly ParticipationStatus[],
): Record<ParticipationStatus, number> {
  const counts: Record<ParticipationStatus, number> = {
    "needs-action": 0,
    accepted: 0,
    declined: 0,
    tentative: 0,
    delegated: 0,
  };
  for (const s of statuses) counts[s]++;
  return counts;
}
