/**
 * Fluent message ids of the window manager for the catalog coverage test:
 * literal ids starting with one of `prefixes` must exist in the English
 * catalogs (`windows.ftl`, `core-windows.ftl`); ids built at runtime are
 * listed by `runtimeIds`.
 */

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = [
  "window-manager-",
  "window-snap-",
  "window-state-issue-",
];

/** Literals with those prefixes that are not message ids. */
export const notMessages: readonly string[] = [
  // Part names (features/windows/skin.ts).
  "window-scrim",
  "window-snap-preview",
];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [];
}
