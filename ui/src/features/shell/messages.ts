/**
 * Fluent message ids of the shell for the catalog coverage test: literal
 * ids starting with one of `prefixes` must exist in the English catalogs
 * (`shell.ftl`, `core-shell.ftl`); ids built at runtime are listed by
 * `runtimeIds`. `shell.test.ts` checks this folder on its own;
 * `../../i18n/coverage.test.ts` picks it up once the integrator lists it.
 */
import { BADGE_SOURCES, badgeLabelId } from "./model";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["shell-", "nav-rail-", "view-action-"];

/** Literals with those prefixes that are not message ids. */
export const notMessages: readonly string[] = [];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [...BADGE_SOURCES.map(badgeLabelId), "shell-rail-badge-count", "shell-rail-badge-many"];
}
