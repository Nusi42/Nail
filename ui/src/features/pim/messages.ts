/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/pim.ftl` (or, for texts the
 * core builds, `locales/en/core-pim.ftl`); ids built at runtime are listed
 * by `runtimeIds`.
 */
import type { PimIssue, PimSourceState } from "../../api/generated";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["pim-"];

/** Literals with those prefixes that are not message ids (part, widget, dialog names). */
export const notMessages: readonly string[] = [];

/** Validation issues the core reports (`PimIssue::message_id`). */
export const PIM_ISSUES: readonly PimIssue[] = [
  "invalid-date",
  "invalid-time",
  "invalid-time-zone",
  "end-before-start",
  "mixed-all-day",
  "invalid-recurrence",
  "invalid-scope",
  "invalid-email",
  "missing-name",
  "invalid-priority",
  "invalid-color",
  "invalid-alarm",
  "invalid-url",
  "task-cycle",
  "too-many",
];

/** Source states (`PimSourceState::message_id`). */
export const SOURCE_STATES: readonly PimSourceState[] = [
  "idle",
  "syncing",
  "offline",
  "auth-error",
  "error",
  "disabled",
];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    ...PIM_ISSUES.map((i) => `pim-issue-${i}`),
    ...SOURCE_STATES.map((s) => `pim-source-state-${s}`),
  ];
}
