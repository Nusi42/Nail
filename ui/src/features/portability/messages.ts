/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/portability.ftl` (texts the
 * contract names — formats, job kinds / phases / states, issues, backup
 * items, diagnostics items — are in `locales/en/core-portability.ftl`);
 * ids built at runtime are listed by `runtimeIds`.
 */

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["portability-"];

/** Literals with those prefixes that are not message ids (part, widget, dialog names). */
export const notMessages: readonly string[] = [];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return ["portability-title", "m4c-not-ready"];
}
