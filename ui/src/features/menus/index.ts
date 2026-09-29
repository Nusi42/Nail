/**
 * Feature slot of M2c WP54 for right-click context menus rendered from
 * `layout.kdl` (`menu "context:<part>" { … }`; contract ADR 0043). The
 * model is `ResolvedMenu` / `ResolvedMenuEntry`; `./resolve.ts` resolves a
 * layout menu like `nail_api::menus::resolve_menu`.
 *
 * Created by WP50 with the resolver. Only WP54 edits the files in this
 * directory (plus `locales/{en,de}/menus.ftl`). The central registry
 * (`../registry.ts`) merges these exports into the app; see `../README.md`.
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

export { contextMenuName, keyContextOf, resolveMenu } from "./resolve";
