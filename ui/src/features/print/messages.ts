/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/print.ftl`; ids built at
 * runtime are listed by `runtimeIds`.
 */
import {
  AGENDA_LAYOUT_MESSAGE_IDS,
  HEADER_DETAIL_MESSAGE_IDS,
  PAGE_ORIENTATION_MESSAGE_IDS,
  PAGE_SIZE_MESSAGE_IDS,
  PRINT_TARGET_ISSUE_MESSAGE_IDS,
} from "../../api/generated";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["print-"];

/** Literals with those prefixes that are not message ids (part, widget, dialog names). */
export const notMessages: readonly string[] = ["print-setup", "remote-images"];

/** The contract's choices (generated from `nail_api::print`). */
export {
  AGENDA_LAYOUT_VALUES as AGENDA_LAYOUTS,
  HEADER_DETAIL_VALUES as HEADER_DETAILS,
  PAGE_ORIENTATION_VALUES as ORIENTATIONS,
  PAGE_SIZE_VALUES as PAGE_SIZES,
} from "../../api/generated";

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    "print-title",
    "print-job-expired",
    "desktop-unavailable",
    ...Object.values(AGENDA_LAYOUT_MESSAGE_IDS),
    ...Object.values(PAGE_SIZE_MESSAGE_IDS),
    ...Object.values(PAGE_ORIENTATION_MESSAGE_IDS),
    ...Object.values(HEADER_DETAIL_MESSAGE_IDS),
    ...Object.values(PRINT_TARGET_ISSUE_MESSAGE_IDS),
    "window-print",
    "print-filter-pdf",
    ...["from", "to", "cc", "reply-to"].map((h) => `print-header-${h}`),
    ...["message", "thread", "event", "agenda"].map((w) => `print-what-${w}`),
  ];
}
