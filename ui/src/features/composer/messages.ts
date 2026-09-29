/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/composer.ftl` (or the M1a
 * `compose.ftl`); ids built at runtime are listed by `runtimeIds`.
 */
import type { ComposerMode } from "../../api/generated";
import { TOOLBAR_IDS } from "./toolbar";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["composer-"];

/** Literals with those prefixes that are not message ids (part names). */
export const notMessages: readonly string[] = [];

const MODES: readonly ComposerMode[] = ["plain", "html", "markdown"];

/** Message ids built at runtime (e.g. `window-<name>` of dialogs). */
export function runtimeIds(): string[] {
  return [
    ...MODES.map((m) => `composer-mode-${m}`),
    ...TOOLBAR_IDS.flatMap((id) => [`composer-format-${id}`, `composer-format-${id}-short`]),
    "composer-field-subject",
  ];
}
