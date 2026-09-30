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
export const effects: Feature["effects"] = [Inspector, SplitWriteBack];
