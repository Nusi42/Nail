/**
 * Feature slot of M5d WP182 (contract ADR 0131): the toolbar customizer —
 * right-click a toolbar → Customize (`menu "context:toolbar"` in
 * `defaults/layout.kdl`); drag buttons between the bar, "More" and a
 * palette of available actions, plug-in actions included; full keyboard
 * alternative.
 *
 * The contract:
 * - Edits are `UpdateLayout` patches (`move-toolbar-item`,
 *   `add-toolbar-item`, `remove-toolbar-item` with `ToolbarSlot` = bar /
 *   more + index; `reset-toolbar` for "Reset to default"), sent with the
 *   layout history's `revision` as `base_revision`.
 * - Undo / Redo: `../../state/editHistory.ts` (`useEditHistory("layout")`,
 *   shared with the layout editor).
 * - Available actions: the layout's `actions`, the registered actions
 *   (`appRegistry.actions` / `actionInfo`) and plug-in commands
 *   (`usePluginLayout`).
 * - The mock core applies this feature's patches through `./mock.ts`
 *   (`applyToolbarEditPatch`, called by the customization mock).
 *
 * WP182 owns this directory, `../../widgets/Toolbar.tsx` +
 * `../../widgets/toolbarModel.ts` (the context menu and the customize
 * overlay of a toolbar), and `locales/{en,de}/toolbar-editor.ftl` (contract
 * ids in `core-editors.ftl`). The central registry (`../registry.ts`) merges
 * these exports into the app; see `../README.md`.
 */
import type { ActionInfo } from "../../actions/palette";
import type { Feature } from "../types";

/** Customizes the toolbar the context menu was opened on. */
export const TOOLBAR_CUSTOMIZE_ACTION = "toolbar.customize";

/** The settings card (`toolbar-editor-settings`); WP182 replaces this placeholder. */
function ToolbarEditorSettings(): null {
  return null;
}

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "toolbar-editor-settings": ToolbarEditorSettings,
};

/** Action ids → handlers. Until WP182 lands the action cannot run. */
export const actions: Feature["actions"] = {
  [TOOLBAR_CUSTOMIZE_ACTION]: {
    run: () => undefined,
    enabled: () => false,
  },
};

/** Command-palette descriptions. */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {};

/** Dialogs by name. */
export const dialogs: Feature["dialogs"] = {};

/** Window model registrations by kind. */
export const windows: NonNullable<Feature["windows"]> = {};

/** Core event handlers. */
export const events: Feature["events"] = [];

/** Background components. */
export const effects: Feature["effects"] = [];
