/**
 * Feature slot of M2c WP54 for right-click context menus rendered from
 * `layout.kdl` (`menu "context:<part>" { … }`; contract ADR 0043). The
 * model is `ResolvedMenu` / `ResolvedMenuEntry`; `./resolve.ts` resolves a
 * layout menu like `nail_api::menus::resolve_menu`.
 *
 * - `ContextMenuHost` (an effect) opens the menu of the part under a
 *   right-click and renders it (`ContextMenu`, keyboard accessible).
 * - `menu.context` opens the menu of the focused element; the default
 *   layout binds it to Shift+F10 and the menu key.
 * - Actions run with the menu's target as context (`contextTarget`): a
 *   message row is selected first, a folder's menu names the folder.
 *
 * Only WP54 edits the files in this directory (plus
 * `locales/{en,de}/menus.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`.
 */
import type { Feature } from "../types";
import { ContextMenuHost } from "./ContextMenuHost";
import { openContextMenu } from "./store";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  /** Opens the context menu of the focused element (Shift+F10, menu key). */
  "menu.context": {
    run: (ctx) => {
      const focused = document.activeElement;
      openContextMenu(ctx.store, ctx.layout, focused, { point: null });
    },
  },
};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};
/** Handlers for the core's events. */
export const events: Feature["events"] = [];
/** Background components. */
export const effects: Feature["effects"] = [ContextMenuHost];

export { contextMenuName, keyContextOf, resolveMenu } from "./resolve";
export { contextTarget, menuStoreFor, openContextMenu } from "./store";
export type { ContextTarget } from "./targets";
export { ariaShortcut, chordText, keyLabel } from "./keyLabels";
