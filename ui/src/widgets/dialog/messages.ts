/**
 * Fluent message ids of the shared dialog pattern for the catalog coverage
 * test (`../../i18n/coverage.test.ts`): literal ids starting with one of
 * `prefixes` must exist in the English catalogs (`dialog.ftl`).
 */
import { UNSAVED_PROMPT } from "../../layout/closeGuard";

/** Prefixes of message ids written literally in these sources. */
export const prefixes: readonly string[] = ["dialog-"];

/** Literals with those prefixes that are not message ids (part names). */
export const notMessages: readonly string[] = [
  // The settings save bar's part (`settings-` is the settings page's prefix).
  "settings-save-bar",
];

/** Message ids built at runtime: the prompt window's title. */
export function runtimeIds(): string[] {
  return [`window-${UNSAVED_PROMPT}`];
}
