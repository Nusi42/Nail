/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/triage.ftl` (or, for the
 * texts the core builds, `core-triage.ftl`); ids built at runtime are
 * listed by `runtimeIds`.
 */
import { JUNK_ISSUES, JUNK_SOURCES, NOTIFICATION_ISSUES, SNOOZE_PRESETS, WEEKDAYS } from "./model";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["junk-", "snooze-", "notification-"];

/** Literals with those prefixes that are not message ids (part names, widget kinds). */
export const notMessages: readonly string[] = [
  "junk-settings",
  "junk-verdict",
  "snooze-list",
  "notification-settings",
];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    ...JUNK_ISSUES.map((i) => `junk-issue-${i}`),
    ...JUNK_SOURCES.map((s) => `junk-source-${s}`),
    ...NOTIFICATION_ISSUES.map((i) => `notification-issue-${i}`),
    ...SNOOZE_PRESETS.map((p) => `snooze-preset-${p}`),
    ...WEEKDAYS.map((d) => `notification-day-${d}`),
    ...["default", "on", "off"].map((c) => `notification-override-${c}`),
    ...["inbox", "all"].map((f) => `notification-folders-${f}`),
    ...["snooze", "snoozed"].map((w) => `window-${w}`),
    "junk-settings-saved",
    "junk-settings-reset-done",
    "notification-settings-saved",
    "notification-test-sent",
    "notification-browser-denied",
    "notification-browser-needed",
  ];
}
