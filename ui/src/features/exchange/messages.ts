/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/exchange.ftl` (or, for texts
 * the core builds, `locales/en/core-exchange.ftl`); ids built at runtime
 * are listed by `runtimeIds`.
 */
import {
  AUTOMATIC_REPLIES_ISSUE_VALUES,
  AUTOMATIC_REPLIES_STATUS_VALUES,
  CATEGORY_COLOR_VALUES,
  EXCHANGE_CATEGORY_ISSUE_VALUES,
  EXCHANGE_ENGINE_VALUES,
  EXCHANGE_MAILBOX_ISSUE_VALUES,
  EXCHANGE_MAILBOX_STATE_VALUES,
  EXCHANGE_RULES_STATE_VALUES,
  EXCHANGE_WARNING_VALUES,
  EXTERNAL_AUDIENCE_VALUES,
  INFERENCE_CLASSIFICATION_VALUES,
  PARTICIPATION_STATUS_VALUES,
  RULE_PLACEMENT_VALUES,
  SSPI_PACKAGE_VALUES,
} from "../../api/generated";
import type {
  AutomaticRepliesIssue,
  CategoryColor,
  ExchangeCategoryIssue,
  ExchangeEngine,
  ExchangeMailboxIssue,
  ExchangeMailboxState,
  ExchangeWarning,
  InferenceClassification,
} from "../../api/generated";
import { EXCHANGE_SETUP_CHOICES, EXCHANGE_SIGN_INS, MAILBOX_SHOW_KEYS } from "./model";
import { DIALOGS, WIDGETS } from "./names";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["exchange-"];

/** Literals with those prefixes that are not message ids (part, widget, dialog names). */
export const notMessages: readonly string[] = [
  ...DIALOGS,
  ...WIDGETS.filter((w) => w.startsWith("exchange-")),
  // Form field names (`data-field`).
  "exchange-host",
  "exchange-server",
  "exchange-url",
  "exchange-server-rules",
  // Ids of the demo data (`mockSeed.ts`).
  "exchange-cal-calendar",
  "exchange-cal-charles",
  "exchange-todo-tasks",
  "exchange-todo-groceries",
  "exchange-todo-flagged",
  "exchange-task-1",
  "exchange-task-2",
  "exchange-task-3",
];

/** Warnings (`ExchangeWarning::message_id`). */
export const WARNINGS: readonly ExchangeWarning[] = EXCHANGE_WARNING_VALUES;

/** Engines (`ExchangeEngine::message_id`). */
export const ENGINES: readonly ExchangeEngine[] = EXCHANGE_ENGINE_VALUES;

/** Focused Inbox tabs (`InferenceClassification::message_id`). */
export const FOCUS: readonly InferenceClassification[] = INFERENCE_CLASSIFICATION_VALUES;

/** Mailbox problems (`ExchangeMailboxIssue::message_id`). */
export const MAILBOX_ISSUES: readonly ExchangeMailboxIssue[] = EXCHANGE_MAILBOX_ISSUE_VALUES;

/** Mailbox states (`ExchangeMailboxState::message_id`). */
export const MAILBOX_STATES: readonly ExchangeMailboxState[] = EXCHANGE_MAILBOX_STATE_VALUES;

/** Category problems (`ExchangeCategoryIssue::message_id`). */
export const CATEGORY_ISSUES: readonly ExchangeCategoryIssue[] = EXCHANGE_CATEGORY_ISSUE_VALUES;

/** Category colours (`CategoryColor::message_id`), `none` first. */
export const CATEGORY_COLORS: readonly CategoryColor[] = CATEGORY_COLOR_VALUES;

/** Automatic-reply problems (`AutomaticRepliesIssue::message_id`). */
export const OOF_ISSUES: readonly AutomaticRepliesIssue[] = AUTOMATIC_REPLIES_ISSUE_VALUES;

/** Texts of the UI built at runtime, by prefix → values. */
const UI_RUNTIME: Record<string, readonly string[]> = {
  "exchange-mailbox-kind-": ["shared", "delegate", "shared-long", "delegate-long"],
  "exchange-mailbox-show-": MAILBOX_SHOW_KEYS,
  "exchange-move-to-": FOCUS,
  "exchange-classify-question-": FOCUS,
  "exchange-oof-status-": AUTOMATIC_REPLIES_STATUS_VALUES,
  "exchange-oof-audience-": EXTERNAL_AUDIENCE_VALUES,
  "exchange-oof-": ["start", "end"],
  "exchange-placement-": RULE_PLACEMENT_VALUES,
  "exchange-placement-short-": RULE_PLACEMENT_VALUES,
  "exchange-rules-state-": EXCHANGE_RULES_STATE_VALUES,
  "exchange-room-": ["free", "busy", "unknown"],
  "exchange-room-response-": PARTICIPATION_STATUS_VALUES,
  "exchange-tasks-kind-": ["todo", "flagged", "tasks"],
  "exchange-setup-type-": EXCHANGE_SETUP_CHOICES,
  "exchange-sign-in-": EXCHANGE_SIGN_INS,
  "exchange-sspi-": SSPI_PACKAGE_VALUES,
  "exchange-push-": ["streaming", "polling", "none"],
};

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    ...Object.entries(UI_RUNTIME).flatMap(([prefix, values]) => values.map((v) => prefix + v)),
    ...DIALOGS.map((d) => `window-${d}`),
    ...WARNINGS.map((w) => `exchange-warning-${w}`),
    ...ENGINES.map((e) => `exchange-engine-${e}`),
    ...FOCUS.map((f) => `exchange-focus-${f}`),
    ...MAILBOX_ISSUES.map((i) => `exchange-mailbox-issue-${i}`),
    ...MAILBOX_STATES.map((s) => `exchange-mailbox-state-${s}`),
    ...CATEGORY_ISSUES.map((i) => `exchange-category-issue-${i}`),
    ...CATEGORY_COLORS.map((c) => `exchange-color-${c}`),
    ...OOF_ISSUES.map((i) => `exchange-oof-issue-${i}`),
  ];
}
