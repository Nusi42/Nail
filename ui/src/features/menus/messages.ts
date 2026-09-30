/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/menus.ftl`; ids built at
 * runtime are listed by `runtimeIds`.
 */
import { NAMED_KEYS } from "./keyLabels";
import { CONTEXT_MENU_PARTS } from "./resolve";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["context-menu-"];

/** Literals with those prefixes that are not message ids (part names). */
export const notMessages: readonly string[] = [];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    ...Object.keys(CONTEXT_MENU_PARTS).map((p) => `context-menu-label-${p.replaceAll(".", "-")}`),
    ...NAMED_KEYS.map((k) => `context-menu-key-${k}`),
  ];
}
