/**
 * Feature slot of M4c WP124 (contract ADR 0094): the command palette —
 * every action id (built-ins, features, layout aliases, plug-in commands)
 * with titles, keywords, shortcuts and context availability
 * (`../../actions/palette.ts`, `Feature.actionInfo`), fuzzy search, recent
 * actions (kept by the core), keyboard-first.
 *
 * Only WP124 edits the files in this directory (plus
 * `locales/{en,de}/palette.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`.
 */
import type { Feature } from "../types";

/** Query keys of this feature. */
export const paletteKeys = {
  all: ["palette"] as const,
  recent: ["palette", "recent"] as const,
};

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers (WP124: `palette.open`). */
export const actions: Feature["actions"] = {};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};

/** Handlers for the core's events. */
export const events: Feature["events"] = [];
/** Background components. */
export const effects: Feature["effects"] = [];
