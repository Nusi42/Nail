/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/scheduling.ftl` (or, for
 * texts the core builds, `locales/en/core-scheduling.ftl`); ids built at
 * runtime are listed by `runtimeIds`.
 */
import type {
  CancelScope,
  FreeBusyIssue,
  InvitationAction,
  InvitationState,
  ItipIssue,
  ItipMethod,
} from "../../api/generated";
import { DIALOGS, FREEBUSY_WIDGET, INVITATION_CARD_WIDGET } from "./names";
import { RESPONSES } from "./model";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["scheduling-", "invitation-", "freebusy-"];

/** Literals with those prefixes that are not message ids (part, widget, dialog names). */
export const notMessages: readonly string[] = [...DIALOGS, INVITATION_CARD_WIDGET, FREEBUSY_WIDGET];

/** Problems with invitation messages the core reports (`ItipIssue::message_id`). */
export const ITIP_ISSUES: readonly ItipIssue[] = [
  "unparsable",
  "unknown-method",
  "no-event",
  "missing-uid",
  "missing-organizer",
  "attendee-count",
  "sender-not-organizer",
  "sender-not-attendee",
  "not-addressed-to-me",
  "several-events",
  "organizer-changed",
  "sent-on-behalf",
  "uninvited-attendee",
];

/** Problems with free/busy questions (`FreeBusyIssue::message_id`). */
export const FREE_BUSY_ISSUES: readonly FreeBusyIssue[] = [
  "attendee-count",
  "invalid-attendee",
  "invalid-range",
  "range-too-long",
];

export const METHODS: readonly ItipMethod[] = [
  "publish",
  "request",
  "reply",
  "add",
  "cancel",
  "refresh",
  "counter",
  "decline-counter",
];

export const STATES: readonly InvitationState[] = [
  "new",
  "current",
  "update",
  "outdated",
  "cancelled",
  "unknown-event",
  "invalid",
];

export const ACTIONS: readonly InvitationAction[] = [
  "accept",
  "tentative",
  "decline",
  "propose-new-time",
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

/** Actions the card runs at once, with a "done" text each. */
export const APPLIED: readonly InvitationAction[] = [
  "add-to-calendar",
  "update-calendar",
  "remove-from-calendar",
  "apply-reply",
  "send-current",
  "accept-counter",
  "decline-counter",
];

export const CANCEL_SCOPES: readonly CancelScope[] = ["meeting", "occurrence", "attendees"];
/** Kinds of free/busy sources (`FreeBusySource.type`). */
export const SOURCES = ["own", "caldav", "exchange", "unknown"] as const;
/** Busy kinds (`BusyKind`). */
export const BUSY = ["tentative", "busy", "unavailable"] as const;
/** The scheduling settings' switches. */
export const SETTING_SWITCHES = [
  "add-invitations",
  "apply-updates",
  "apply-replies",
  "apply-cancellations",
  "confirm-replies",
] as const;

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    ...ITIP_ISSUES.map((i) => `itip-issue-${i}`),
    ...FREE_BUSY_ISSUES.map((i) => `freebusy-issue-${i}`),
    ...METHODS.map((m) => `invitation-kind-${m}`),
    ...STATES.map((s) => `invitation-state-${s}`),
    ...ACTIONS.map((a) => `invitation-action-${a}`),
    ...APPLIED.map((a) => `invitation-applied-${a}`),
    ...["room", "resource"].map((k) => `invitation-attendee-${k}`),
    ...RESPONSES.flatMap((r) => [`invitation-done-${r}`, `invitation-done-${r}-sent`]),
    ...RESPONSES.map((r) => `scheduling-answer-${r}`),
    ...CANCEL_SCOPES.map((s) => `scheduling-cancel-scope-${s}`),
    ...SOURCES.map((s) => `freebusy-source-${s}`),
    ...BUSY.map((b) => `freebusy-kind-${b}`),
    ...SETTING_SWITCHES.flatMap((s) => [`scheduling-setting-${s}`, `scheduling-setting-${s}-hint`]),
    ...DIALOGS.map((d) => `window-${d}`),
    ...["individual", "room", "resource"].map((k) => `scheduling-kind-${k}`),
    ...["series", "occurrence"].map((s) => `scheduling-scope-${s}`),
    ...["everyone", "chosen"].map((s) => `scheduling-send-to-${s}`),
  ];
}
