/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/palette.ftl`; ids built at
 * runtime are listed by `runtimeIds`.
 */
import {
  ACTION_AVAILABILITY_MESSAGE_IDS,
  ACTION_AVAILABILITY_VALUES,
  type ActionAvailability,
} from "../../api/generated";
import { builtinMessageIds } from "./builtinInfo";
import { PALETTE_DIALOG } from "./names";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["palette-"];

/** Literals with those prefixes that are not message ids (part, widget, dialog names). */
export const notMessages: readonly string[] = [];

/** Every `ActionAvailability` (generated from `nail_api::palette`). */
export { ACTION_AVAILABILITY_VALUES as ACTION_AVAILABILITIES } from "../../api/generated";

/** Fluent id of an availability (`action-availability-*`). */
export function availabilityMessageId(availability: ActionAvailability): string {
  return ACTION_AVAILABILITY_MESSAGE_IDS[availability];
}

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    "palette-title",
    "palette-invalid-action",
    `window-${PALETTE_DIALOG}`,
    ...ACTION_AVAILABILITY_VALUES.map(availabilityMessageId),
    ...builtinMessageIds(),
  ];
}
