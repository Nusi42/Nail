/**
 * Feature slot of M5c WP171 (contract ADR 0116): the window manager — move
 * by the title bar, resize from 8 handles, maximize / restore (double-click),
 * snap to the left or right half, keyboard move / resize (arrow keys and
 * Shift+arrow keys on a focused title bar, Alt+F7 / Alt+F8 equivalents),
 * focus order and z-order, minimum sizes, staying on screen when the page
 * shrinks, and remembered geometry.
 *
 * The contract: window ids, specs and checks in `../../state/windows.ts`
 * (mirrors `nail_api::windows`; `resolveWindowSpec` = defaults →
 * `Feature.windows` → the layout's `window` properties), remembered state in
 * `../../state/windowState.ts` (`useWindowState`, `useSetWindowState`, per
 * kind), z-order = `UiState.windows` (topmost last; `focusWindow`), the
 * window instance of a component = `WindowInstanceContext`. WP171 also owns
 * `../../layout/` (the window layer `WindowLayer.tsx` and the view
 * components). Texts: `locales/{en,de}/windows.ftl` (contract ids in
 * `core-windows.ftl`).
 *
 * Only WP171 edits the files in this directory. The central registry
 * (`../registry.ts`) merges these exports into the app; see `../README.md`.
 */
import type { ActionInfo } from "../../actions/palette";
import { windowStatesKey } from "../../state/windowState";
import type { Feature } from "../types";
import { runWindowCommand } from "./commands";

export { WINDOWS_SKIN_CSS, WINDOW_PARTS } from "./skin";
export { runWindowCommand, setWindowCommandHandler, type WindowCommand } from "./commands";

/** Action ids of the window manager. */
export const WINDOW_CYCLE_ACTION = "window.cycle";
export const WINDOW_MAIN_ACTION = "window.main";
export const WINDOW_MAXIMIZE_ACTION = "window.maximize";
export const WINDOW_RESET_ACTION = "window.reset-positions";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  // Ctrl+F6 (built into the window layer; bindable like any action).
  [WINDOW_CYCLE_ACTION]: {
    run: (ctx) => {
      runWindowCommand(ctx.store, { type: "cycle" });
    },
    enabled: (ctx) => ctx.store.getState().windows.length > 0,
  },
  // Ctrl+Shift+F6.
  [WINDOW_MAIN_ACTION]: {
    run: (ctx) => {
      runWindowCommand(ctx.store, { type: "main" });
    },
    enabled: (ctx) => ctx.store.getState().windows.length > 0,
  },
  [WINDOW_MAXIMIZE_ACTION]: {
    run: (ctx) => {
      runWindowCommand(ctx.store, { type: "maximize" });
    },
    enabled: (ctx) => ctx.store.getState().windows.length > 0,
  },
  // Forgets every remembered window position; open windows go back to their default places.
  [WINDOW_RESET_ACTION]: {
    run: async (ctx) => {
      const { states } = await ctx.client.request({ type: "ResetWindowStates" }, "WindowStates");
      ctx.queryClient.setQueryData(windowStatesKey, states);
      runWindowCommand(ctx.store, { type: "reset" });
    },
  },
};

/** Command-palette descriptions. */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
  [WINDOW_CYCLE_ACTION]: {
    title: "window-manager-action-cycle",
    keywords: "window-manager-action-cycle-keywords",
    icon: "app-window",
  },
  [WINDOW_MAIN_ACTION]: {
    title: "window-manager-action-main",
    keywords: "window-manager-action-main-keywords",
    icon: "app-window",
  },
  [WINDOW_MAXIMIZE_ACTION]: {
    title: "window-manager-action-maximize",
    keywords: "window-manager-action-maximize-keywords",
    icon: "maximize-2",
  },
  [WINDOW_RESET_ACTION]: {
    title: "window-manager-action-reset",
    keywords: "window-manager-action-reset-keywords",
    icon: "app-window",
  },
};

/** Dialogs by name. */
export const dialogs: Feature["dialogs"] = {};

/** Window model registrations by kind. */
export const windows: NonNullable<Feature["windows"]> = {};

/** Core event handlers. */
export const events: Feature["events"] = [];

/** Background components. */
export const effects: Feature["effects"] = [];
