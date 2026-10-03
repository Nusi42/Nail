/**
 * Feature slot of M5b WP161 (contract ADR 0109): the shell — the
 * navigation rail (`nav-rail`, from the layout's `rail`), the view host
 * (`view-host`, the active view's window tree from `views`), view switching
 * (`view.show "<view>"`, `view.next`, `view.previous`; shortcuts through the
 * layout's aliases `view.mail` … and `ctrl+1` …), the badges (unread mail,
 * confirmations), and Calendar / Contacts / Tasks as full views with the
 * existing PIM widgets.
 *
 * The contract's rules are in `../../layout/views.ts` (mirrors
 * `nail_api::views`); the active view is `UiState.activeView`
 * (`../../state/uiStore.ts`), remembered in the core's UI state under
 * `shell.view` (`../../state/uiState.ts`). Texts: `locales/{en,de}/shell.ftl`
 * (contract ids in `core-shell.ftl`).
 *
 * The contract stage ships working fallbacks so the actions run with any
 * layout: `view.show` opens the view's window when the main window has no
 * `view-host` (the default layout until WP165), and the two widgets render
 * nothing yet.
 */
import type { ActionInfo } from "../../actions/palette";
import type { ActionHandler } from "../../actions/registry";
import type { JsonValue } from "../../api/generated";
import {
  VIEW_NEXT_ACTION,
  VIEW_PREVIOUS_ACTION,
  VIEW_SHOW_ACTION,
  currentView,
  hostsViews,
  stepView,
  viewDef,
} from "../../layout/views";
import type { ActionContext } from "../../actions/registry";
import type { Feature } from "../types";
import { NavRail, ViewHost } from "./widgets";

/** Shows a view: in the `view-host`, else by opening its window. */
export function showView(ctx: ActionContext, view: string): void {
  const def = viewDef(ctx.layout, view);
  if (def === null) return;
  const store = ctx.store.getState();
  store.setActiveView(view);
  if (hostsViews(ctx.layout)) return;
  if (def.window === "main") {
    // The main window is the view: close what covers it.
    for (const window of [...store.windows].reverse()) store.closeWindow(window);
  } else {
    store.openWindow(def.window);
  }
}

const viewArg = (args: readonly JsonValue[]): string | null =>
  typeof args[0] === "string" ? args[0] : null;

const step = (delta: 1 | -1): ActionHandler => ({
  run: (ctx) => {
    const now = currentView(ctx.layout, ctx.store.getState().activeView);
    const next = now === null ? null : stepView(ctx.layout, now, delta);
    if (next !== null) showView(ctx, next);
  },
  enabled: (ctx) => Object.keys(ctx.layout.views).length > 1,
});

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "nav-rail": NavRail,
  "view-host": ViewHost,
};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  [VIEW_SHOW_ACTION]: {
    run: (ctx, args) => {
      const view = viewArg(args);
      if (view !== null) showView(ctx, view);
    },
    enabled: (ctx, args) => {
      const view = viewArg(args);
      return view !== null && viewDef(ctx.layout, view) !== null;
    },
  },
  [VIEW_NEXT_ACTION]: step(1),
  [VIEW_PREVIOUS_ACTION]: step(-1),
};

/** Command-palette descriptions (the per-view aliases are listed instead of `view.show`). */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
  [VIEW_SHOW_ACTION]: { title: "view-action-show", hidden: true },
  [VIEW_NEXT_ACTION]: { title: "view-action-next" },
  [VIEW_PREVIOUS_ACTION]: { title: "view-action-previous" },
};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};
/** Handlers for core events. */
export const events: Feature["events"] = [];
/** Background components. */
export const effects: Feature["effects"] = [];
