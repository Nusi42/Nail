/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/sync.ftl`; ids built at
 * runtime are listed by `runtimeIds`.
 */
import { BODY_MODES } from "./model";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["sync-"];

/** Literals with those prefixes that are not message ids (part names, widget kinds). */
export const notMessages: readonly string[] = [
  // The status bar's sync item (`data-item`).
  "sync-progress",
  "sync-folder-policies",
  "sync-message-body",
  "sync-policy",
];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    ...BODY_MODES.map((m) => `sync-bodies-${m}`),
    ...BODY_MODES.map((m) => `sync-choice-${m}`),
    ...["enabled", "idle"].flatMap((f) => ["default", "yes", "no"].map((v) => `sync-${f}-${v}`)),
    "window-sync-folder",
    "sync-message-body-failed",
    "sync-message-body-missing",
    "sync-message-body-partial",
    "sync-message-body-fetching",
  ];
}
