/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/triage.ftl` (or, for the
 * texts the core builds, `core-triage.ftl`); ids built at runtime are
 * listed by `runtimeIds`.
 */
import {
  JUNK_ISSUE_MESSAGE_IDS,
  NOTIFICATION_ISSUE_MESSAGE_IDS,
  NOTIFY_FOLDERS_VALUES,
} from "../../api/generated";
import { JUNK_SOURCES, OVERRIDE_CHOICES, SNOOZE_PRESETS, WEEKDAYS } from "./model";

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
    ...Object.values(JUNK_ISSUE_MESSAGE_IDS),
    ...JUNK_SOURCES.map((s) => `junk-source-${s}`),
    ...Object.values(NOTIFICATION_ISSUE_MESSAGE_IDS),
    ...SNOOZE_PRESETS.map((p) => `snooze-preset-${p}`),
    ...WEEKDAYS.map((d) => `notification-day-${d}`),
    ...OVERRIDE_CHOICES.map((c) => `notification-override-${c}`),
    ...NOTIFY_FOLDERS_VALUES.map((f) => `notification-folders-${f}`),
    ...["snooze", "snoozed"].map((w) => `window-${w}`),
    "junk-settings-saved",
    "junk-settings-reset-done",
    "notification-settings-saved",
    "notification-test-sent",
    "notification-browser-denied",
    "notification-browser-needed",
  ];
}
