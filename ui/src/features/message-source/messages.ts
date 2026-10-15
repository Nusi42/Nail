/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/source.ftl`; ids built at
 * runtime are listed by `runtimeIds`.
 */
import { MESSAGE_SOURCE_KIND, SOURCE_VIEWS } from "./names";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["source-"];

/** Literals with those prefixes that are not message ids (import / export wizard fields). */
export const notMessages: readonly string[] = ["source-kind", "source-path"];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [`window-${MESSAGE_SOURCE_KIND}`, ...SOURCE_VIEWS.map((v) => `source-view-${v}`)];
}
