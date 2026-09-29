/**
 * The shape of a feature slot (`src/features/<name>/index.ts`). See
 * `README.md` in this directory.
 */
import type { QueryClient } from "@tanstack/react-query";
import type { ComponentType } from "react";
import type { ActionHandler } from "../actions/registry";
import type { NailClient } from "../api/client";
import type { Event as NailEvent } from "../api/generated";
import type { DialogRegistry } from "../layout/dialogRegistry";
import type { WidgetRegistry } from "../layout/widgetRegistry";
import type { UiStore } from "../state/uiStore";

/** What an event handler of a feature may use. */
export interface FeatureEventContext {
  client: NailClient;
  queryClient: QueryClient;
  store: UiStore;
}

/** Called for every event the core pushes. */
export type FeatureEventHandler = (event: NailEvent, ctx: FeatureEventContext) => void;

/** Everything a feature contributes to the app. */
export interface Feature {
  /** Layout widget kinds → components (also register each kind in nail-config). */
  widgets: WidgetRegistry;
  /** Action ids → handlers (toolbars, menus and key bindings refer to the ids). */
  actions: Readonly<Record<string, ActionHandler>>;
  /**
   * Dialogs opened with `window.open "<name>"` when the layout has no
   * `window "<name>"`: small, fixed-purpose windows (confirmations,
   * "token shown once"). Anything the user should be able to arrange
   * belongs in `layout.kdl` as a window of widgets instead.
   */
  dialogs: DialogRegistry;
  /** Handlers for the events the core pushes. */
  events: readonly FeatureEventHandler[];
  /**
   * Components mounted once inside the app shell (they render nothing) for
   * background work, e.g. reporting the UI context to the core.
   */
  effects: readonly ComponentType[];
}
