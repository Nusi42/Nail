/**
 * Feature slot of M1b WP23 (MCP): MCP client settings (tokens, grants, revoke, audit log) and the indicator shown while an agent reads the UI context.
 *
 * Only WP23 edits the files in this directory. The central registry
 * (`../registry.ts`) merges these exports into the app; see `../README.md`.
 */
import type { Feature } from "../types";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {};

/** Named dialogs (opened with `window.open "<name>"`). */
export const dialogs: Feature["dialogs"] = {};

/** Handlers for core events. */
export const events: Feature["events"] = [];

/** Background components mounted once in the app shell. */
export const effects: Feature["effects"] = [];
