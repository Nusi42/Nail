/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en` (`folders.ftl`; the core's
 * reasons are in `core-folders.ftl`); ids built at runtime are listed by
 * `runtimeIds`.
 */
import { FOLDER_DELETE_DIALOG, FOLDER_MOVE_DIALOG, FOLDER_NAME_DIALOG } from "./names";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["folders-", "folder-issue-"];

/** Literals with those prefixes that are not message ids. */
export const notMessages: readonly string[] = [];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [FOLDER_NAME_DIALOG, FOLDER_MOVE_DIALOG, FOLDER_DELETE_DIALOG].map((k) => `window-${k}`);
}
