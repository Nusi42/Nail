/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/rules.ftl`; ids built at
 * runtime are listed by `runtimeIds`.
 */
import type { RuleRunCause, RuleRunState } from "../../api/generated";
import { FOLDER_ROLES } from "../../api/contract";
import { ADDRESS_FIELDS, AUTH_METHODS, AUTH_RESULTS, FLAGS } from "./ConditionEditor";
import { ACTION_KINDS, CONDITION_KINDS, GROUP_KINDS, TEXT_OPS, TRIGGER_KINDS } from "./model";
import { STARTER_IDS } from "./starters";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["rules-", "rule-"];

/** Literals with those prefixes that are not message ids (part names). */
export const notMessages: readonly string[] = [
  "rule-manager",
  "rule-name",
  "rule-enabled",
  "rule-text",
  // The list's filter and the editor's view switch (`name`).
  "rule-filter",
  "rule-view",
  // Query key of the run history.
  "rule-runs",
  // R6: the editor's settings draft key and its question windows (`./windows.ts`).
  "rule-editor",
  "rule-delete",
  "rule-run",
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
    // R1: the "Run rule" menu item and the labels of the rules menu (`./runMenu.ts`).
    "rules-action-run-menu",
    ...["all", "off", "none"].map((k) => `rules-menu-${k}`),
    ...CONDITION_KINDS.map((k) => `rules-cond-${k}`),
    ...ACTION_KINDS.map((k) => `rules-do-${k}`),
    ...TEXT_OPS.map((k) => `rules-op-${k}`),
    ...TRIGGER_KINDS.map((k) => `rules-trigger-${k}`),
    ...TRIGGER_KINDS.map((k) => `rules-when-${k}`),
    ...ADDRESS_FIELDS.map((k) => `rules-field-${k}`),
    ...FLAGS.map((k) => `rules-flag-${k}`),
    ...FOLDER_ROLES.map((k) => `rules-role-${k}`),
    ...AUTH_METHODS.map((k) => `rules-auth-${k}`),
    ...AUTH_RESULTS.map((k) => `rules-auth-result-${k}`),
    ...["greater", "less"].flatMap((k) => [`rules-size-${k}`, `rules-age-${k}`]),
    ...["before", "after"].map((k) => `rules-date-${k}`),
    ...GROUP_KINDS.map((k) => `rules-group-${k}`),
    ...["set", "unset"].map((k) => `rules-flag-state-${k}`),
    ...["flag", "read", "notify"].flatMap((k) => [
      `rules-set-${k}-label`,
      `rules-set-${k}-on`,
      `rules-set-${k}-off`,
    ]),
    ...RUN_STATES.map((s) => `rules-run-state-${s}`),
    ...RUN_CAUSES.map((c) => `rules-run-cause-${c}`),
    // R2: the starter rules of the empty rules window (`./starters.ts`).
    ...STARTER_IDS.flatMap((id) => [`rules-starter-${id}-name`, `rules-starter-${id}-hint`]),
  ];
}
