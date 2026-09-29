/**
 * Feature slot of M2c WP54 for per-folder sync policies (full / headers
 * only / recent N days) and bodies downloaded on demand (contract
 * ADR 0043): the folder's sync settings, the "not downloaded" marker in the
 * list and reader, "download now".
 *
 * Created empty by WP50. Only WP54 edits the files in this directory (plus
 * `crates/nail-config/src/widgets/sync.rs` for widget kinds and
 * `locales/{en,de}/sync.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`.
 */
import type { Feature } from "../types";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};
/** Action ids → handlers. */
export const actions: Feature["actions"] = {};
/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};
/** Handlers for the core's events. */
export const events: Feature["events"] = [];
/** Background components. */
export const effects: Feature["effects"] = [];
