/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/print.ftl`; ids built at
 * runtime are listed by `runtimeIds`.
 */
import type { AgendaLayout, HeaderDetail, PageOrientation, PageSize } from "../../api/generated";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["print-"];

/** Literals with those prefixes that are not message ids (part, widget, dialog names). */
export const notMessages: readonly string[] = ["print-setup", "remote-images"];

/** The contract's choices (mirror `nail_api::print`). */
export const AGENDA_LAYOUTS: readonly AgendaLayout[] = ["day", "week", "month", "list"];
export const PAGE_SIZES: readonly PageSize[] = ["a4", "a5", "letter", "legal"];
export const ORIENTATIONS: readonly PageOrientation[] = ["portrait", "landscape"];
export const HEADER_DETAILS: readonly HeaderDetail[] = ["basic", "full"];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    "print-title",
    "print-job-expired",
    "desktop-unavailable",
    ...AGENDA_LAYOUTS.map((l) => `print-agenda-layout-${l}`),
    ...PAGE_SIZES.map((s) => `print-page-size-${s}`),
    ...ORIENTATIONS.map((o) => `print-orientation-${o}`),
    ...HEADER_DETAILS.map((h) => `print-headers-${h}`),
    ...["invalid-date", "end-before-start", "range-too-long"].map((i) => `print-target-issue-${i}`),
    "window-print",
    "print-filter-pdf",
    ...["from", "to", "cc", "reply-to"].map((h) => `print-header-${h}`),
    ...["message", "thread", "event", "agenda"].map((w) => `print-what-${w}`),
  ];
}
