/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/rules.ftl`; ids built at
 * runtime are listed by `runtimeIds`.
 */
import type { RuleRunCause, RuleRunState } from "../../api/generated";
import { SPECIAL_USE_ROLES } from "../../widgets/folderNames";
import { ADDRESS_FIELDS, AUTH_METHODS, AUTH_RESULTS, FLAGS } from "./ConditionEditor";
import { ACTION_KINDS, CONDITION_KINDS, TEXT_OPS, TRIGGER_KINDS } from "./model";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["rules-", "rule-"];

/** Literals with those prefixes that are not message ids (part names). */
export const notMessages: readonly string[] = [
  "rule-manager",
  "rule-name",
  "rule-enabled",
  "rule-text",
  "rule-up",
  "rule-down",
  // Query key of the run history.
  "rule-runs",
  // Prefixes of the on/off action choices (`<prefix>-label`, `-on`, `-off`).
  "rules-set-flag",
  "rules-set-read",
  "rules-set-notify",
];

const RUN_STATES: readonly RuleRunState[] = ["running", "done", "failed", "undone"];
const RUN_CAUSES: readonly RuleRunCause[] = ["arrival", "on-send", "schedule", "manual"];

/** Message ids built at runtime (e.g. `window-<name>` of dialogs). */
export function runtimeIds(): string[] {
  return [
    "window-rules",
    "rules-action-open",
    "rules-action-create-from-message",
    ...CONDITION_KINDS.map((k) => `rules-cond-${k}`),
    ...ACTION_KINDS.map((k) => `rules-do-${k}`),
    ...TEXT_OPS.map((k) => `rules-op-${k}`),
    ...TRIGGER_KINDS.map((k) => `rules-trigger-${k}`),
    ...TRIGGER_KINDS.map((k) => `rules-when-${k}`),
    ...ADDRESS_FIELDS.map((k) => `rules-field-${k}`),
    ...FLAGS.map((k) => `rules-flag-${k}`),
    ...SPECIAL_USE_ROLES.map((k) => `rules-role-${k}`),
    ...AUTH_METHODS.map((k) => `rules-auth-${k}`),
    ...AUTH_RESULTS.map((k) => `rules-auth-result-${k}`),
    ...["greater", "less"].flatMap((k) => [`rules-size-${k}`, `rules-age-${k}`]),
    ...["before", "after"].map((k) => `rules-date-${k}`),
    ...["all", "any", "not"].map((k) => `rules-group-${k}`),
    ...["set", "unset"].map((k) => `rules-flag-state-${k}`),
    ...["flag", "read", "notify"].flatMap((k) => [
      `rules-set-${k}-label`,
      `rules-set-${k}-on`,
      `rules-set-${k}-off`,
    ]),
    ...RUN_STATES.map((s) => `rules-run-state-${s}`),
    ...RUN_CAUSES.map((c) => `rules-run-cause-${c}`),
  ];
}
