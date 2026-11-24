/**
 * Pure helpers of the quick look, the compact event editor and the reminder
 * toast (R7, ADR 0246): lengths as text, the reminder chip's choices, who
 * answered what, the user's own attendee entry and the call link to join.
 * No React, no requests: unit-tested in `eventLook.test.ts`.
 */
import { MINUTES_PER_DAY } from "../../api/generated";
import type { Alarm, Attendee, ConferenceLink, ParticipationStatus } from "../../api/generated";
import type { MessageArgs } from "../../i18n/localizer";
import { answerCounts, isMine } from "../scheduling/model";
import { instantMs } from "./time";

type T = (id: string, args?: MessageArgs) => string;

/** Minutes in an hour. */
export const MINUTES_PER_HOUR = 60;

/** The icon of an event's place (R8, ADR 0251: the editor's place field and the quick look). */
export const PLACE_ICON = "map-pin";
/** The icon of an event's call link (the quick look's Join). */
export const CALL_ICON = "video";

/** A length split into whole days, hours and minutes (never negative). */
export function lengthParts(minutes: number): { days: number; hours: number; minutes: number } {
  const total = Math.max(0, Math.round(minutes));
  return {
    days: Math.floor(total / MINUTES_PER_DAY),
    hours: Math.floor((total % MINUTES_PER_DAY) / MINUTES_PER_HOUR),
    minutes: total % MINUTES_PER_HOUR,
  };
}

/**
 * A length as text in the UI language ("1 hour", "1 hour, 30 minutes",
 * "2 days"): one Fluent message per unit, joined as a unit list of the
 * locale (`Intl.ListFormat`).
 */
export function lengthText(t: T, locale: string, minutes: number): string {
  const parts = lengthParts(minutes);
  const words = [
    parts.days > 0 ? t("pim-look-length-days", { count: parts.days }) : null,
    parts.hours > 0 ? t("pim-look-length-hours", { count: parts.hours }) : null,
    parts.minutes > 0 || (parts.days === 0 && parts.hours === 0)
      ? t("pim-look-length-minutes", { count: parts.minutes })
      : null,
  ].filter((w): w is string => w !== null);
  return new Intl.ListFormat(locale, { type: "unit", style: "narrow" }).format(words);
}

/** Minutes between two instants (RFC 3339). */
export function minutesOf(startUtc: string, endUtc: string): number {
  return Math.round((instantMs(endUtc) - instantMs(startUtc)) / 60_000);
}

// ---------------------------------------------------------------------------
// The reminder chip

/** Minutes before the start the reminder chip offers (0 = at the start). */
export const REMINDER_PRESETS = [0, 5, 10, 15, 30, MINUTES_PER_HOUR, MINUTES_PER_DAY] as const;

/** What the reminder chip shows: none, one preset, or "custom" (anything else). */
export type ReminderChoice = "none" | "custom" | `${(typeof REMINDER_PRESETS)[number]}`;

/** The chip's values in their order (custom only when the alarms are custom). */
export function reminderChoices(current: ReminderChoice): ReminderChoice[] {
  const presets = REMINDER_PRESETS.map((m) => String(m) as ReminderChoice);
  return ["none", ...presets, ...(current === "custom" ? (["custom"] as const) : [])];
}

/** Whether an alarm is a plain display reminder `minutes` before the start. */
function isPreset(alarm: Alarm, minutes: number): boolean {
  return (
    alarm.action === "display" &&
    alarm.trigger.type === "relative" &&
    alarm.trigger.related === "start" &&
    alarm.trigger.offset_secs === -minutes * 60
  );
}

/** The chip's value for an event's alarms. */
export function reminderChoiceOf(alarms: readonly Alarm[]): ReminderChoice {
  if (alarms.length === 0) return "none";
  const [only] = alarms;
  if (alarms.length > 1 || only === undefined) return "custom";
  const preset = REMINDER_PRESETS.find((m) => isPreset(only, m));
  return preset === undefined ? "custom" : (String(preset) as ReminderChoice);
}

/**
 * The alarms after choosing `choice` on the chip: none, or one display
 * reminder before the start (keeping the first alarm's uid, so a server
 * sees a change rather than a new alarm). "custom" keeps them.
 */
export function alarmsForChoice(choice: ReminderChoice, alarms: readonly Alarm[]): Alarm[] {
  if (choice === "custom") return [...alarms];
  if (choice === "none") return [];
  return [
    {
      uid: alarms[0]?.uid ?? null,
      action: "display",
      trigger: { type: "relative", offset_secs: -Number(choice) * 60, related: "start" },
      description: null,
      repeat: null,
      acknowledged: null,
    },
  ];
}

/** The chip's text for a choice. */
export function reminderText(t: T, locale: string, choice: ReminderChoice): string {
  if (choice === "none") return t("pim-look-reminder-none");
  if (choice === "custom") return t("pim-look-reminder-custom");
  if (choice === "0") return t("pim-look-reminder-at-start");
  return t("pim-look-reminder-before", { length: lengthText(t, locale, Number(choice)) });
}

// ---------------------------------------------------------------------------
// People

/** The user's own attendee entry, if the user is invited. */
export function myAttendee(
  attendees: readonly Attendee[],
  mine: readonly string[],
): Attendee | null {
  return attendees.find((a) => isMine(a.email, mine)) ?? null;
}

/** The answers people gave, in the order the summary lists them (zero counts left out). */
const SUMMARY_ORDER = ["accepted", "tentative", "declined", "needs-action"] as const;

/** Message ids of the answer counts. */
const ANSWER_IDS: Record<(typeof SUMMARY_ORDER)[number], string> = {
  accepted: "pim-look-answers-accepted",
  tentative: "pim-look-answers-tentative",
  declined: "pim-look-answers-declined",
  "needs-action": "pim-look-answers-pending",
};

/** "3 people · 2 accepted, 1 not answered" (zero counts left out). */
export function peopleSummary(t: T, locale: string, statuses: readonly ParticipationStatus[]) {
  const counts = answerCounts(statuses);
  // A delegated attendee has not answered for the person asked.
  const tally = { ...counts, "needs-action": counts["needs-action"] + counts.delegated };
  const answers = SUMMARY_ORDER.filter((s) => tally[s] > 0).map((s) =>
    t(ANSWER_IDS[s], { count: tally[s] }),
  );
  const people = t("pim-look-people-count", { count: statuses.length });
  if (answers.length === 0) return people;
  return t("pim-look-people-summary", {
    people,
    answers: new Intl.ListFormat(locale, { type: "unit", style: "short" }).format(answers),
  });
}

// ---------------------------------------------------------------------------
// Calls

/** The first call link that can be opened safely (https), else `null`. */
export function joinLinkOf(conferences: readonly ConferenceLink[]): string | null {
  return conferences.find((c) => /^https:\/\//i.test(c.uri.trim()))?.uri.trim() ?? null;
}
