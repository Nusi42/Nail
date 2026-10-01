/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/gmail.ftl` (or, for texts
 * the core builds, `locales/en/core-gmail.ftl`); ids built at runtime are
 * listed by `runtimeIds`.
 */
import type {
  GmailCategory,
  GmailLabelIssue,
  GmailLabelListVisibility,
  GmailMessageListVisibility,
} from "../../api/generated";
import { DIALOGS } from "./names";
import { LABEL_COLORS } from "./palette";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["gmail-"];

/** Literals with those prefixes that are not message ids (part, widget, dialog names). */
export const notMessages: readonly string[] = [...DIALOGS];

/** Label problems the core reports (`GmailLabelIssue::message_id`). */
export const LABEL_ISSUES: readonly GmailLabelIssue[] = [
  "missing-name",
  "name-too-long",
  "invalid-name",
  "invalid-color",
  "duplicate-name",
];

/** Inbox categories (`GmailCategory::message_id`). */
export const CATEGORIES: readonly GmailCategory[] = [
  "primary",
  "social",
  "promotions",
  "updates",
  "forums",
];

const LIST: readonly GmailLabelListVisibility[] = ["show", "show-if-unread", "hide"];
const ON_MESSAGES: readonly GmailMessageListVisibility[] = ["show", "hide"];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    ...LABEL_ISSUES.map((i) => `gmail-label-issue-${i}`),
    ...CATEGORIES.map((c) => `gmail-category-${c}`),
    ...["none", "current", ...LABEL_COLORS.map((c) => c.name)].map((c) => `gmail-color-${c}`),
    ...LIST.map((v) => `gmail-list-visibility-${v}`),
    ...ON_MESSAGES.map((v) => `gmail-message-visibility-${v}`),
    ...DIALOGS.map((d) => `window-${d}`),
  ];
}
