/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/scheduling.ftl` (or, for
 * texts the core builds, `locales/en/core-scheduling.ftl`); ids built at
 * runtime are listed by `runtimeIds`.
 */
import type { FreeBusyIssue, ItipIssue } from "../../api/generated";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["scheduling-", "invitation-", "freebusy-"];

/** Literals with those prefixes that are not message ids (part, widget, dialog names). */
export const notMessages: readonly string[] = [];

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
];

/** Problems with free/busy questions (`FreeBusyIssue::message_id`). */
export const FREE_BUSY_ISSUES: readonly FreeBusyIssue[] = [
  "attendee-count",
  "invalid-attendee",
  "invalid-range",
  "range-too-long",
];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    ...ITIP_ISSUES.map((i) => `itip-issue-${i}`),
    ...FREE_BUSY_ISSUES.map((i) => `freebusy-issue-${i}`),
  ];
}
