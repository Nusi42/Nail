/**
 * Feature slot of M2d WP62 for plug-ins (contract ADR 0050): the renderer
 * of plug-in component trees (dialogs, panels), plug-in commands in
 * toolbars / menus / keys, notifications, the install dialog with its
 * permissions, the plug-in manager (enable / disable, grants, logs,
 * settings) and placements proposed through the merge dialog.
 *
 * Only WP62 edits the files in this directory (plus
 * `crates/nail-config/src/widgets/plugins.rs`,
 * `crates/nail-config/src/plugins.rs` and `locales/{en,de}/plugins.ftl`).
 * The central registry (`../registry.ts`) merges these exports into the
 * app; see `../README.md`.
 */
import type { Feature, FeatureEventHandler } from "../types";
import { PluginPanel } from "./PluginPanel";

/** Query keys of this feature. */
export const pluginKeys = {
  all: ["plugins"] as const,
};

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "plugin-panel": PluginPanel,
};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};

/** Installed plug-ins changed: refetch what depends on them. */
export const handlePluginsEvent: FeatureEventHandler = (event, { queryClient }) => {
  if (event.type === "PluginsChanged") {
    void queryClient.invalidateQueries({ queryKey: pluginKeys.all });
  }
};

/** Handlers for the core's events. */
export const events: Feature["events"] = [handlePluginsEvent];
/** Background components. */
export const effects: Feature["effects"] = [];
