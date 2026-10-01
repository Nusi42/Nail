/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/pim.ftl` (or, for texts the
 * core builds, `locales/en/core-pim.ftl`); ids built at runtime are listed
 * by `runtimeIds`.
 */
import type {
  DuplicateReason,
  PimIssue,
  PimSourceKind,
  PimSourceState,
  PimItemSync,
} from "../../api/generated";
import {
  ALARM_ACTIONS,
  ALARM_UNITS,
  ATTENDEE_ROLES,
  CONTACT_KINDS,
  EDIT_SCOPES,
  EVENT_STATUSES,
  TASK_STATUSES,
  TRANSPARENCIES,
  VISIBILITIES,
} from "./constants";
import { LIST_FIELDS, SINGLE_FIELDS } from "./merge";
import { DIALOGS } from "./names";
import { SIMPLE_FREQUENCIES } from "./recurrence";
import { VIEW_KINDS } from "./time";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["pim-"];

/** Literals with those prefixes that are not message ids (dialog names). */
export const notMessages: readonly string[] = [...DIALOGS];

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

export const SOURCE_KINDS_ALL: readonly PimSourceKind[] = ["local", "ics", "caldav", "carddav"];
export const SYNC_STATES: readonly PimItemSync[] = ["synced", "pending", "conflict"];
export const DUPLICATE_REASONS: readonly DuplicateReason[] = ["email", "phone", "name"];
export const PARTSTATS = [
  "needs-action",
  "accepted",
  "declined",
  "tentative",
  "delegated",
] as const;
export const ALARM_WHEN = ["before-start", "after-start", "before-end", "after-end"] as const;
export const PRIORITY_KEYS = ["none", "high", "medium", "low"] as const;

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    ...PIM_ISSUES.map((i) => `pim-issue-${i}`),
    ...SOURCE_STATES.map((s) => `pim-source-state-${s}`),
    ...SOURCE_KINDS_ALL.map((k) => `pim-source-kind-${k}`),
    ...SYNC_STATES.map((s) => `pim-sync-${s}`),
    ...DIALOGS.map((d) => `window-${d}`),
    ...VIEW_KINDS.map((v) => `pim-view-${v}`),
    ...EVENT_STATUSES.map((s) => `pim-status-${s}`),
    ...TRANSPARENCIES.map((s) => `pim-transparency-${s}`),
    ...VISIBILITIES.map((s) => `pim-visibility-${s}`),
    ...ATTENDEE_ROLES.map((r) => `pim-role-${r}`),
    ...PARTSTATS.map((p) => `pim-partstat-${p}`),
    ...ALARM_UNITS.map((u) => `pim-alarm-unit-${u}`),
    ...ALARM_WHEN.map((w) => `pim-alarm-${w}`),
    ...ALARM_ACTIONS.map((a) => `pim-alarm-action-${a}`),
    ...EDIT_SCOPES.flatMap((s) => [`pim-scope-${s}`, `pim-delete-scope-${s}`]),
    ...SIMPLE_FREQUENCIES.map((f) => `pim-recurrence-freq-${f}`),
    ...CONTACT_KINDS.map((k) => `pim-contact-kind-${k}`),
    ...DUPLICATE_REASONS.map((r) => `pim-duplicate-${r}`),
    ...[...SINGLE_FIELDS, ...LIST_FIELDS, "addresses"].map((f) => `pim-merge-field-${f}`),
    ...PRIORITY_KEYS.map((p) => `pim-priority-${p}`),
    ...TASK_STATUSES.map((s) => `pim-task-status-${s}`),
  ];
}
