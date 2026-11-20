/**
 * Fluent message ids of the shared pickers for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en` (`pickers.ftl`); ids built at
 * runtime are listed by `runtimeIds`.
 */

/** Prefixes of message ids written literally in the pickers' sources. */
export const prefixes: readonly string[] = ["pickers-"];

/** Literals with those prefixes that are not message ids. */
export const notMessages: readonly string[] = [];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [];
}
