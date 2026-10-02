/**
 * Feature slot of M4c WP124 (contract ADR 0094): the command palette —
 * every action id (built-ins, features, layout aliases, plug-in commands)
 * with titles, keywords, shortcuts and context availability
 * (`../../actions/palette.ts`, `Feature.actionInfo`), fuzzy search, recent
 * actions (kept by the core), keyboard-first.
 *
 * Contributions: dialog `command-palette`; actions `palette.open` (bind it
 * in `layout.kdl`, default Ctrl+K / Ctrl+Shift+P) and
 * `palette.clear-recent`; descriptions of the built-in actions and of the
 * features that predate `Feature.actionInfo` (`builtinInfo.ts`); theme base
 * CSS `PALETTE_BASE_CSS`.
 *
 * The palette itself (`Palette.tsx`) imports the app registry to list
 * every action, so it is loaded lazily here: the registry imports this
 * file, never the other way round while modules load.
 *
 * Only WP124 edits the files in this directory (plus
 * `locales/{en,de}/palette.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`.
 */
import { createElement, lazy, Suspense } from "react";
import type { ActionInfo } from "../../actions/palette";
import type { DialogProps } from "../../layout/dialogRegistry";
import type { Feature } from "../types";
import { contextsOf } from "./contexts";
import { PALETTE_CLEAR_RECENT_ACTION, PALETTE_DIALOG, PALETTE_OPEN_ACTION } from "./names";
import { paletteStoreFor } from "./store";

export * from "./names";
export { BUILTIN_INFO, HIDDEN_ACTIONS } from "./builtinInfo";
export { contextsOf } from "./contexts";
export { searchPalette } from "./fuzzy";
export { paletteStoreFor } from "./store";
export { PALETTE_BASE_CSS } from "./parts";

/** Query keys of this feature. */
export const paletteKeys = {
  all: ["palette"] as const,
  recent: ["palette", "recent"] as const,
};

const LazyPalette = lazy(async () => ({
  default: (await import("./Palette")).CommandPaletteDialog,
}));

/** Loads the palette's code ahead of time (tests, idle time). */
export function loadPalette(): Promise<unknown> {
  return import("./Palette");
}

/** The palette dialog: its code loads on first use. */
export function PaletteDialog(props: DialogProps) {
  return createElement(Suspense, { fallback: null }, createElement(LazyPalette, props));
}

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  /**
   * Opens the palette with the contexts of where the user is now (focus
   * moves into the palette); a second press closes it.
   */
  [PALETTE_OPEN_ACTION]: {
    run: (ctx) => {
      const state = ctx.store.getState();
      if (state.windows.includes(PALETTE_DIALOG)) {
        state.closeWindow(PALETTE_DIALOG);
        return;
      }
      paletteStoreFor(ctx.store)
        .getState()
        .setContexts([...contextsOf(state, false)]);
      state.openWindow(PALETTE_DIALOG);
    },
  },
  /** Forgets the recently used actions. */
  [PALETTE_CLEAR_RECENT_ACTION]: {
    run: async (ctx) => {
      const answer = await ctx.client.request({ type: "ClearRecentActions" }, "RecentActions");
      ctx.queryClient.setQueryData(paletteKeys.recent, answer.recent);
    },
  },
};

/** Command-palette descriptions of the palette's own actions. */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
  [PALETTE_OPEN_ACTION]: { title: "palette-action-open", hidden: true },
  [PALETTE_CLEAR_RECENT_ACTION]: {
    title: "palette-action-clear-recent",
    keywords: "palette-action-clear-recent-keywords",
  },
};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {
  [PALETTE_DIALOG]: PaletteDialog,
};

/** Handlers for the core's events. */
export const events: Feature["events"] = [];
/** Background components. */
export const effects: Feature["effects"] = [];
