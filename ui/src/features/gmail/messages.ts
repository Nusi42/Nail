/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/gmail.ftl` (or, for texts
 * the core builds, `locales/en/core-gmail.ftl`); ids built at runtime are
 * listed by `runtimeIds`.
 */
import {
  GMAIL_CATEGORY_VALUES,
  GMAIL_LABEL_ISSUE_VALUES,
  GMAIL_LABEL_LIST_VISIBILITY_VALUES,
  GMAIL_MESSAGE_LIST_VISIBILITY_VALUES,
} from "../../api/generated";
import type {
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
export const LABEL_ISSUES: readonly GmailLabelIssue[] = GMAIL_LABEL_ISSUE_VALUES;

const LIST: readonly GmailLabelListVisibility[] = GMAIL_LABEL_LIST_VISIBILITY_VALUES;
const ON_MESSAGES: readonly GmailMessageListVisibility[] = GMAIL_MESSAGE_LIST_VISIBILITY_VALUES;

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    ...LABEL_ISSUES.map((i) => `gmail-label-issue-${i}`),
    ...GMAIL_CATEGORY_VALUES.map((c) => `gmail-category-${c}`),
    ...["none", "current", ...LABEL_COLORS.map((c) => c.name)].map((c) => `gmail-color-${c}`),
    ...LIST.map((v) => `gmail-list-visibility-${v}`),
    ...ON_MESSAGES.map((v) => `gmail-message-visibility-${v}`),
    ...DIALOGS.map((d) => `window-${d}`),
  ];
}
