/**
 * Fluent message ids of pop-out windows for the catalog coverage test:
 * literal ids starting with one of `prefixes` must exist in the English
 * catalogs (`popout.ftl`, `core-windows.ftl`); ids built at runtime are
 * listed by `runtimeIds`.
 */

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["popout-", "pop-out-"];

/** Literals with those prefixes that are not message ids. */
export const notMessages: readonly string[] = ["popout-probe"];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [];
}
