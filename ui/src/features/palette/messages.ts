/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/palette.ftl`; ids built at
 * runtime are listed by `runtimeIds`.
 */
import type { ActionAvailability } from "../../api/generated";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["palette-"];

/** Literals with those prefixes that are not message ids (part, widget, dialog names). */
export const notMessages: readonly string[] = [];

/** Every `ActionAvailability` (mirrors `nail_api::palette`). */
export const ACTION_AVAILABILITIES: readonly ActionAvailability[] = [
  "global",
  "list",
  "reader",
  "composer",
  "calendar",
  "contacts",
  "tasks",
  "desktop",
];

/** Fluent id of an availability (`action-availability-*`). */
export function availabilityMessageId(availability: ActionAvailability): string {
  return `action-availability-${availability}`;
}

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    "palette-title",
    "palette-invalid-action",
    ...ACTION_AVAILABILITIES.map(availabilityMessageId),
  ];
}
