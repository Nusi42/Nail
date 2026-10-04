/**
 * Feature slot of M5d WP181 (contract ADR 0131): customize-layout mode —
 * pane headers with drag handles, dock and swap targets and a hide / show
 * eye; the widget palette ("Add to layout") and the presets (Three panes,
 * Reader below, Wide reader, Focus); Undo, Reset to default and Done; a full
 * keyboard alternative.
 *
 * The contract:
 * - Edits are `UpdateLayout` patches (`move-pane`, `swap-panes`,
 *   `add-widget`, `remove-node`, `set-hidden`, `apply-preset`,
 *   `reset-window`; `nail_api::editing` / `LayoutPatch`), sent with the
 *   layout history's `revision` as `base_revision` (`conflict` = the file
 *   changed on disk: reload, then retry).
 * - Undo / Redo: `../../state/editHistory.ts` (`useEditHistory("layout")`,
 *   `useEditHistoryStep("layout")`; entries are labelled `edit-kind-*`).
 * - Presets: `ResolvedLayout.presets` (`defaults/layout.kdl` `presets`); a
 *   preset is offered for a breakpoint only when the breakpoint has every
 *   pane it names (`PresetNode`, `pane_ids`).
 * - Hidden nodes: `LayoutNode.hidden`; the renderer skips them unless the
 *   layout chrome shows them (`../../layout/LayoutChrome.ts`:
 *   `LayoutChromeContext` — `showHidden` and `renderNode` wrap every node
 *   with its `LayoutNodeRef`; this feature provides it in customize mode).
 * - The mock core applies this feature's patches through `./mock.ts`
 *   (`applyLayoutEditPatch`, called by the customization mock; never
 *   re-exported here, so the mock stays out of the app bundle).
 *
 * Modules: `model.ts` (panes, drop zones, patches, presets, the palette),
 * `store.ts` (the session), `chrome.tsx` (the layout chrome), `Pane.tsx`,
 * `EditorFrame.tsx`, `Palette.tsx`, `dnd.ts` (pointer dragging),
 * `SettingsCard.tsx` (`layout-editor-settings`), `start.ts`,
 * `useLayoutEdit.ts`, `skin.ts` (`LAYOUT_EDITOR_SKIN_CSS`), `messages.ts`.
 *
 * WP181 owns this directory, `../../layout/LayoutRenderer.tsx` +
 * `../../layout/LayoutChrome.ts`, the placement of the widget
 * `layout-editor-settings` (and WP182's `toolbar-editor-settings`) on the
 * settings page in `defaults/layout.kdl` (then the nail-api fixture
 * `default-layout.json`; the kinds are registered in
 * `crates/nail-config/src/widgets/editors.rs` by the contract), and
 * `locales/{en,de}/layout-editor.ftl` (contract ids in `core-editors.ftl`).
 * The central registry (`../registry.ts`) merges these exports into the
 * app; see `../README.md`.
 */
import type { ActionInfo } from "../../actions/palette";
import type { Feature } from "../types";
import { editorChrome } from "./chrome";
import { LayoutEditorSettings } from "./SettingsCard";
import { customizeTarget, startCustomizing } from "./start";
import { editorStore, setChromeFactory, stopEditing } from "./store";

export { LAYOUT_EDITOR_SKIN_CSS } from "./skin";
export { startCustomizing, customizeTarget } from "./start";
export { startEditing, stopEditing, useEditor } from "./store";

setChromeFactory(editorChrome);

/** Opens customize-layout mode (`defaults/layout.kdl` `actions`). */
export const LAYOUT_CUSTOMIZE_ACTION = "layout.customize";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "layout-editor-settings": LayoutEditorSettings,
};

/**
 * Action ids → handlers: `layout.customize` customizes the shown view's
 * window (again: Done).
 */
export const actions: Feature["actions"] = {
  [LAYOUT_CUSTOMIZE_ACTION]: {
    run: (ctx) => {
      if (editorStore.getState().window !== null) stopEditing();
      else startCustomizing(ctx.store, ctx.layout);
    },
    enabled: (ctx) =>
      editorStore.getState().window !== null || customizeTarget(ctx.layout, ctx.store) !== null,
  },
};

/** Command-palette descriptions. */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
  [LAYOUT_CUSTOMIZE_ACTION]: {
    title: "layout-editor-action-customize",
    keywords: "layout-editor-action-customize-keywords",
  },
};

/** The settings page's search index. */
export const settings: NonNullable<Feature["settings"]> = {
  "layout-editor-settings": {
    labels: [
      "layout-editor-settings-title",
      "layout-editor-settings-window",
      "layout-editor-action-customize",
      "layout-editor-reset",
      "layout-editor-presets",
    ],
    keywords: "layout-editor-settings-keywords",
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
