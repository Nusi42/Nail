/**
 * The shape of a feature slot (`src/features/<name>/index.ts`). See
 * `README.md` in this directory.
 */
import type { QueryClient } from "@tanstack/react-query";
import type { ComponentType } from "react";
import type { ActionInfo } from "../actions/palette";
import type { ActionHandler } from "../actions/registry";
import type { NailClient } from "../api/client";
import type { Event as NailEvent } from "../api/generated";
import type { DialogRegistry } from "../layout/dialogRegistry";
import type { SettingsWidgetInfo } from "../layout/settingsIndex";
import type { WidgetRegistry } from "../layout/widgetRegistry";
import type { WindowTitleRegistry } from "../layout/windowSpecs";
import type { UiStore } from "../state/uiStore";
import type { WindowRegistry } from "../state/windows";

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
   * Command-palette descriptions of this feature's actions (title, keywords,
   * where they apply; M4c, `../actions/palette.ts`). Optional: actions
   * without one are described by the palette feature or not listed.
   */
  actionInfo?: Readonly<Record<string, ActionInfo>>;
  /**
   * The settings page's search index (M5b, ADR 0109): per settings widget
   * kind this feature registers, the Fluent ids of every setting label it
   * shows (`../layout/settingsIndex.ts`). Optional.
   */
  settings?: Readonly<Record<string, SettingsWidgetInfo>>;
  /**
   * Dialogs opened with `window.open "<name>"` when the layout has no
   * `window "<name>"`: small, fixed-purpose windows (confirmations,
   * "token shown once"). Anything the user should be able to arrange
   * belongs in `layout.kdl` as a window of widgets instead.
   */
  dialogs: DialogRegistry;
  /**
   * The window model of this feature's windows (M5c, ADR 0116): by window
   * kind (a dialog name, or a layout window the feature owns), modal or
   * not, resizable, minimum / default size, pop-out, instances
   * (`../state/windows.ts`, `WindowRegistration`). Windows are non-modal by
   * default; register confirmations with `{ confirmation: true }` (the only
   * modal windows). The layout's `window` properties win over this.
   * Optional.
   */
  windows?: WindowRegistry;
  /**
   * Window titles by kind from the UI state (M5c): a composer window shows
   * its subject. `null` = the kind's `window-<kind>` text. Optional.
   */
  windowTitles?: WindowTitleRegistry;
  /** Handlers for the events the core pushes. */
  events: readonly FeatureEventHandler[];
  /**
   * Components mounted once inside the app shell (they render nothing) for
   * background work, e.g. reporting the UI context to the core.
   */
  effects: readonly ComponentType[];
}
