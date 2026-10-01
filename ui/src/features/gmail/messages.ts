/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/gmail.ftl` (or, for texts
 * the core builds, `locales/en/core-gmail.ftl`); ids built at runtime are
 * listed by `runtimeIds`.
 */
import type { GmailCategory, GmailLabelIssue } from "../../api/generated";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["gmail-"];

/** Literals with those prefixes that are not message ids (part, widget, dialog names). */
export const notMessages: readonly string[] = [];

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

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    ...LABEL_ISSUES.map((i) => `gmail-label-issue-${i}`),
    ...CATEGORIES.map((c) => `gmail-category-${c}`),
  ];
}
