/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/triage.ftl` (or, for the
 * texts the core builds, `core-triage.ftl`); ids built at runtime are
 * listed by `runtimeIds`.
 */
import { JUNK_ISSUES, JUNK_SOURCES, NOTIFICATION_ISSUES } from "./model";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["junk-", "snooze-", "notification-"];

/** Literals with those prefixes that are not message ids (part names). */
export const notMessages: readonly string[] = [];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    ...JUNK_ISSUES.map((i) => `junk-issue-${i}`),
    ...JUNK_SOURCES.map((s) => `junk-source-${s}`),
    ...NOTIFICATION_ISSUES.map((i) => `notification-issue-${i}`),
  ];
}
