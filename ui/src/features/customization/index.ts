/**
 * Feature slot of M1b WP24 (customization): the inspector, layout write-back
 * (dividers now; toolbars, keys and columns through `writeBack.ts`), the
 * merge dialog, the config error list and saved searches (smart folders).
 *
 * Only WP24 edits the files in this directory. The central registry
 * (`../registry.ts`) merges these exports into the app; see `../README.md`.
 */
import type { Feature, FeatureEventHandler } from "../types";
import { ConfigErrors, ConfigErrorsDialog } from "./ConfigErrors";
import { ColumnEditor, KeyBindingEditor, ToolbarEditor } from "./Editors";
import { Inspector } from "./Inspector";
import { MergeDialog } from "./MergeDialog";
import { SmartFolders, savedSearchesKey } from "./SmartFolders";
import { SplitWriteBack } from "./SplitWriteBack";
import { DIALOGS } from "./names";
import { customizationStore } from "./state";
import { LayoutMigration } from "./LayoutMigration";

export { DIALOGS };

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "config-errors": ConfigErrors,
  "smart-folders": SmartFolders,
  // M2c WP54: the Customize tab's editors (write-back through layout patches).
  "toolbar-editor": ToolbarEditor,
  "key-binding-editor": KeyBindingEditor,
  "column-editor": ColumnEditor,
};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  /** Turns the inspector's element picker on or off. */
  "inspector.toggle": {
    run: () => {
      const { inspecting, setInspecting } = customizationStore.getState();
      setInspecting(!inspecting);
    },
  },
  /** Opens the merge dialog (apply a shared theme or layout). */
  "customize.merge": {
    run: (ctx) => ctx.store.getState().openWindow(DIALOGS.merge),
  },
  /** Opens the list of configuration errors. */
  "customize.errors": {
    run: (ctx) => ctx.store.getState().openWindow(DIALOGS.errors),
  },
};

/** Named dialogs (opened with `window.open "<name>"`). */
export const dialogs: Feature["dialogs"] = {
  [DIALOGS.merge]: MergeDialog,
  [DIALOGS.errors]: ConfigErrorsDialog,
};

/**
 * The window model of this feature's windows (M5c, ADR 0116): tool windows
 * are non-modal with their default and minimum sizes; only confirmations
 * (`confirmation: true`) are modal. The layout's `window` properties win.
 */
export const windows: NonNullable<Feature["windows"]> = {
  [DIALOGS.merge]: { size: { width: 1000, height: 720 }, min_size: { width: 640, height: 480 } },
  [DIALOGS.errors]: { size: { width: 720, height: 480 }, min_size: { width: 420, height: 280 } },
};

/** Keeps the error list and the saved searches current. */
export const handleCustomizationEvent: FeatureEventHandler = (event, { queryClient }) => {
  customizationStore.getState().applyEvent(event);
  if (event.type === "SavedSearchesChanged") {
    void queryClient.invalidateQueries({ queryKey: savedSearchesKey });
  }
};

/** Handlers for core events. */
export const events: Feature["events"] = [handleCustomizationEvent];

/** Background components mounted once in the app shell. */
export const effects: Feature["effects"] = [Inspector, SplitWriteBack, LayoutMigration];

/**
 * Settings labels per widget kind, for the settings search (M5b WP163, ADR
 * 0109): Fluent ids of the setting labels each widget shows.
 */
export const settings: NonNullable<Feature["settings"]> = {
  "config-errors": { labels: ["config-error-list-label"] },
  "toolbar-editor": {
    labels: [
      "customize-toolbar-tab",
      "customize-toolbar-label",
      "customize-toolbar-choose",
      "customize-toolbar-add",
      "customize-toolbar-reset",
    ],
  },
  "key-binding-editor": {
    labels: [
      "customize-keys-tab",
      "customize-keys-label",
      "customize-keys-table",
      "customize-keys-add",
      "customize-keys-context",
    ],
    keywords: "settings-keywords-key-binding-editor",
  },
  "column-editor": {
    labels: [
      "customize-columns-tab",
      "customize-columns-label",
      "customize-columns-items",
      "customize-columns-add",
    ],
  },
};
