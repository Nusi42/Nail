/**
 * UI state of the plug-in feature, one store per UI store: the plug-in
 * dialogs open in this UI, the notifications shown, and how the install
 * dialog starts (package or development folder).
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type {
  LocalizedMessage,
  PluginDialog,
  PluginNotification,
  PluginNotificationLevel,
} from "../../api/generated";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";

/** A notification on screen: a plug-in's own, or one about a stopped plug-in. */
export type Toast =
  | { kind: "plugin"; id: string; notification: PluginNotification }
  | {
      kind: "stopped";
      id: string;
      pluginId: string;
      level: PluginNotificationLevel;
      message: LocalizedMessage;
    };

/** Most notifications on screen at once (older ones make room). */
export const MAX_TOASTS = 5;

export type InstallMode = "package" | "dev";

export interface PluginsState {
  /** Open dialogs, oldest first. */
  dialogs: PluginDialog[];
  toasts: Toast[];
  installMode: InstallMode;
  /** Opens a dialog, or updates the one with the same id. */
  openDialog(dialog: PluginDialog): void;
  closeDialog(dialogId: string): void;
  /** Closes every dialog of a plug-in (it stopped). */
  closePluginDialogs(pluginId: string): void;
  pushToast(toast: Toast): void;
  dismissToast(id: string): void;
  setInstallMode(mode: InstallMode): void;
}

const stores = new WeakMap<UiStore, StoreApi<PluginsState>>();

export function createPluginsStore(): StoreApi<PluginsState> {
  return createStore<PluginsState>()((set) => ({
    dialogs: [],
    toasts: [],
    installMode: "package",
    openDialog: (dialog) =>
      set((s) => {
        const index = s.dialogs.findIndex((d) => d.dialog_id === dialog.dialog_id);
        if (index < 0) return { dialogs: [...s.dialogs, dialog] };
        const dialogs = [...s.dialogs];
        dialogs[index] = dialog;
        return { dialogs };
      }),
    closeDialog: (dialogId) =>
      set((s) => ({ dialogs: s.dialogs.filter((d) => d.dialog_id !== dialogId) })),
    closePluginDialogs: (pluginId) =>
      set((s) => ({ dialogs: s.dialogs.filter((d) => d.plugin_id !== pluginId) })),
    pushToast: (toast) =>
      set((s) => ({
        toasts: [...s.toasts.filter((t) => t.id !== toast.id), toast].slice(-MAX_TOASTS),
      })),
    dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
    setInstallMode: (installMode) => set({ installMode }),
  }));
}

/** The plug-in store of a UI store. */
export function pluginsStoreFor(ui: UiStore): StoreApi<PluginsState> {
  let store = stores.get(ui);
  if (!store) {
    store = createPluginsStore();
    stores.set(ui, store);
  }
  return store;
}

export function usePluginsState<T>(selector: (state: PluginsState) => T): T {
  return useStore(pluginsStoreFor(useUiStoreApi()), selector);
}
