/**
 * The mail widgets' own registrations (M5b WP162, contract ADR 0109),
 * merged by `../features/registry.ts` like a feature slot: the
 * `list-header` widget (search with the Ctrl+K hint, title with counts,
 * filter chips `nail_api::views::ListFilter` → `UiState.listFilter`), and
 * the actions WP162 adds (`list.filter "<filter>"`,
 * `folders.toggle-section "<id>"`, …) with their palette descriptions and
 * icons (`actionInfo`). The widgets that existed before M5b stay in
 * `./index.ts` (`builtinWidgets`). Rules shared with the core:
 * `./folderPane.ts` (sections), `../layout/overflow.ts` (toolbar "More").
 *
 * Contract stage: the list header renders nothing yet.
 */
import type { ActionInfo } from "../actions/palette";
import type { Feature } from "../features/types";

/** The message list header (`list-header`); WP162 replaces this. */
export function ListHeader() {
  return null;
}

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "list-header": ListHeader,
};
/** Action ids → handlers. */
export const actions: Feature["actions"] = {};
/** Command-palette descriptions and icons of these actions. */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {};
/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};
/** Handlers for core events. */
export const events: Feature["events"] = [];
/** Background components. */
export const effects: Feature["effects"] = [];
