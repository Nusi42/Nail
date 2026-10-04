/**
 * Feature slot of M5d WP182 (contract ADR 0131): the toolbar customizer —
 * right-click a toolbar → Customize (`menu "context:toolbar"` in
 * `defaults/layout.kdl`, opened by the toolbar widget itself); drag buttons
 * between the bar, "More" and a palette of available actions, plug-in
 * actions included; full keyboard alternative; Undo / Redo through the
 * shared `layout` history and Reset to default.
 *
 * - `toolbar.customize [toolbar]` opens the window
 *   `toolbar-customizer:<toolbar>` (`./ids.ts`); without an argument it
 *   customizes the toolbar that has focus, else the window lets the user
 *   pick one (the command palette, Settings).
 * - Edits are `UpdateLayout` patches (`move-toolbar-item`,
 *   `add-toolbar-item`, `remove-toolbar-item` with `ToolbarSlot` = bar /
 *   more + index; `reset-toolbar` for "Reset to default"), sent with the
 *   layout history's `revision` as `base_revision` (`./model.ts`,
 *   `./ToolbarCustomizer.tsx`).
 * - The settings card `toolbar-editor-settings` lists the toolbars
 *   (`./Settings.tsx`).
 * - The mock core applies the patches through `./mock.ts`
 *   (`applyToolbarEditPatch`, called by the customization mock).
 * - The look: `./skin.ts` (`TOOLBAR_CUSTOMIZER_SKIN_CSS`, for the theme
 *   compiler's skin list).
 *
 * WP182 owns this directory, `../../widgets/Toolbar.tsx` +
 * `../../widgets/toolbarModel.ts` (the toolbar's context menu), and
 * `locales/{en,de}/toolbar-editor.ftl` (contract ids in `core-editors.ftl`).
 * The central registry (`../registry.ts`) merges these exports into the
 * app; see `../README.md`.
 */
import type { ActionInfo } from "../../actions/palette";
import type { JsonValue } from "../../api/generated";
import type { Feature } from "../types";
import { TOOLBAR_CUSTOMIZE_ACTION, TOOLBAR_CUSTOMIZER_KIND, customizerWindow } from "./ids";
import { ToolbarEditorSettings } from "./Settings";
import { ToolbarCustomizer } from "./ToolbarCustomizer";

export { TOOLBAR_CUSTOMIZE_ACTION, TOOLBAR_CUSTOMIZER_KIND, customizerWindow } from "./ids";

/**
 * The toolbar an invocation customizes: its argument, else the toolbar
 * around the focused element, else none (the window offers a choice).
 */
export function toolbarToCustomize(
  args: readonly JsonValue[],
  focused: Element | null,
): string | null {
  const [first] = args;
  if (typeof first === "string" && first !== "") return first;
  const bar = focused?.closest<HTMLElement>('[data-part="toolbar"][data-toolbar]');
  return bar?.dataset.toolbar ?? null;
}

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "toolbar-editor-settings": ToolbarEditorSettings,
};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  [TOOLBAR_CUSTOMIZE_ACTION]: {
    run: (ctx, args) => {
      const toolbar = toolbarToCustomize(args, document.activeElement);
      ctx.store.getState().openWindow(customizerWindow(toolbar));
    },
  },
};

/** Command-palette descriptions. */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
  [TOOLBAR_CUSTOMIZE_ACTION]: {
    title: "toolbar-editor-action-customize",
    keywords: "toolbar-editor-action-keywords",
    icon: "settings-2",
  },
};

/** The settings page's search index. */
export const settings: NonNullable<Feature["settings"]> = {
  "toolbar-editor-settings": {
    labels: ["toolbar-editor-settings-title", "toolbar-editor-settings-customize"],
    keywords: "toolbar-editor-action-keywords",
  },
};

/** Dialogs by name. */
export const dialogs: Feature["dialogs"] = {
  [TOOLBAR_CUSTOMIZER_KIND]: ToolbarCustomizer,
};

/** Window model registrations by kind: one non-modal window per toolbar. */
export const windows: NonNullable<Feature["windows"]> = {
  [TOOLBAR_CUSTOMIZER_KIND]: {
    modal: false,
    size: { width: 820, height: 620 },
    min_size: { width: 480, height: 400 },
    instances: "multiple",
    pop_out: false,
  },
};

/** Core event handlers. */
export const events: Feature["events"] = [];

/** Background components. */
export const effects: Feature["effects"] = [];
