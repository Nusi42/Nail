/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/exchange.ftl` (or, for texts
 * the core builds, `locales/en/core-exchange.ftl`); ids built at runtime
 * are listed by `runtimeIds`.
 */
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
export const WARNINGS: readonly ExchangeWarning[] = [
  "ews-retired-online",
  "focused-inbox-unavailable",
  "rooms-need-consent",
  "throttled",
];

/** Engines (`ExchangeEngine::message_id`). */
export const ENGINES: readonly ExchangeEngine[] = ["graph", "ews"];

/** Focused Inbox tabs (`InferenceClassification::message_id`). */
export const FOCUS: readonly InferenceClassification[] = ["focused", "other"];

/** Mailbox problems (`ExchangeMailboxIssue::message_id`). */
export const MAILBOX_ISSUES: readonly ExchangeMailboxIssue[] = [
  "missing-address",
  "invalid-address",
  "own-mailbox",
  "duplicate",
  "empty-name",
  "nothing-shown",
];

/** Mailbox states (`ExchangeMailboxState::message_id`). */
export const MAILBOX_STATES: readonly ExchangeMailboxState[] = [
  "pending",
  "synced",
  "access-denied",
  "not-found",
  "error",
];

/** Category problems (`ExchangeCategoryIssue::message_id`). */
export const CATEGORY_ISSUES: readonly ExchangeCategoryIssue[] = [
  "missing-name",
  "name-too-long",
  "invalid-name",
  "duplicate-name",
];

/** Category colours (`CategoryColor::message_id`), `none` first. */
export const CATEGORY_COLORS: readonly CategoryColor[] = [
  "none",
  ...Array.from({ length: 25 }, (_, i) => `preset${i}` as CategoryColor),
];

/** Automatic-reply problems (`AutomaticRepliesIssue::message_id`). */
export const OOF_ISSUES: readonly AutomaticRepliesIssue[] = [
  "empty-internal-reply",
  "empty-external-reply",
  "reply-too-long",
  "invalid-time",
  "missing-schedule",
  "end-before-start",
];

/** Texts of the UI built at runtime, by prefix → values. */
const UI_RUNTIME: Record<string, readonly string[]> = {
  "exchange-mailbox-kind-": ["shared", "delegate", "shared-long", "delegate-long"],
  "exchange-mailbox-show-": ["mail", "calendar", "contacts", "tasks"],
  "exchange-move-to-": FOCUS,
  "exchange-classify-question-": FOCUS,
  "exchange-oof-status-": ["disabled", "always-enabled", "scheduled"],
  "exchange-oof-audience-": ["none", "contacts-only", "all"],
  "exchange-oof-": ["start", "end"],
  "exchange-placement-": ["server", "client", "not-expressible"],
  "exchange-placement-short-": ["server", "client", "not-expressible"],
  "exchange-rules-state-": ["disabled", "pending", "syncing", "in-sync", "failed"],
  "exchange-room-": ["free", "busy", "unknown"],
  "exchange-room-response-": ["needs-action", "accepted", "declined", "tentative", "delegated"],
  "exchange-tasks-kind-": ["todo", "flagged", "tasks"],
  "exchange-setup-type-": ["auto", "graph", "ews"],
  "exchange-sign-in-": ["windows-current", "windows-account", "password", "oauth2"],
  "exchange-sspi-": ["negotiate", "kerberos", "ntlm"],
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
