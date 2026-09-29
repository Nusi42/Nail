/**
 * Feature slot of M2c WP54 for server-side rules and vacation replies
 * (contract ADR 0043): where each rule runs (server / Nail / Nail only,
 * with the reasons), the ManageSieve state per account and the vacation
 * reply editor.
 *
 * Created empty by WP50. Only WP54 edits the files in this directory (plus
 * `crates/nail-config/src/widgets/sieve.rs` for widget kinds and
 * `locales/{en,de}/sieve.ftl`). The central registry (`../registry.ts`)
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
