/**
 * UI state of the plug-in feature, one store per UI store: the plug-in
 * dialogs open in this UI, the notifications shown, how the install
 * dialog starts (package or development folder), composer edits waiting
 * for the composer (M4a) and the send veto shown (M4a).
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type {
  ActionAttribution,
  ComposerEdit,
  DraftId,
  LocalizedMessage,
  OutboxEntry,
  PluginDialog,
  PluginNotification,
  PluginNotificationLevel,
  PluginSendVeto,
  SendVetoId,
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

/** A composer edit asked for by a plug-in or an agent, waiting for the composer. */
export interface PendingEdit {
  /** Increases with every edit (the order they arrived in). */
  seq: number;
  draftId: DraftId;
  by: ActionAttribution;
  edit: ComposerEdit;
}

/** Edits the composer has not applied yet are kept up to this many. */
export const MAX_PENDING_EDITS = 20;
/** Applied edits remembered for "edited by". */
export const MAX_RECENT_EDITS = 5;

/** A send stopped by before-send hooks, waiting for "Send anyway" or "Keep editing". */
export interface PendingVeto {
  draftId: DraftId;
  vetoId: SendVetoId;
  vetoes: PluginSendVeto[];
  /** Settles the send: the outbox entry, or `null` when the user keeps editing. */
  settle: (entry: OutboxEntry | null) => void;
}

export interface PluginsState {
  /** Open dialogs, oldest first. */
  dialogs: PluginDialog[];
  toasts: Toast[];
  installMode: InstallMode;
  /** Composer edits not yet applied, oldest first. */
  edits: PendingEdit[];
  /** Edits the composer applied lately (for "edited by"), oldest first. */
  recent: PendingEdit[];
  /** The send veto shown, if any. */
  veto: PendingVeto | null;
  /** Opens a dialog, or updates the one with the same id. */
  openDialog: (dialog: PluginDialog) => void;
  closeDialog: (dialogId: string) => void;
  /** Closes every dialog of a plug-in (it stopped). */
  closePluginDialogs: (pluginId: string) => void;
  pushToast: (toast: Toast) => void;
  dismissToast: (id: string) => void;
  setInstallMode: (mode: InstallMode) => void;
  /** Queues a composer edit. */
  pushEdit: (edit: Omit<PendingEdit, "seq">) => void;
  /** Takes (removes) the queued edits of a draft, oldest first. */
  takeEdits: (draftId: DraftId) => PendingEdit[];
  /** Shows a send veto (a shown one is settled as "keep editing" first). */
  showVeto: (veto: PendingVeto | null) => void;
}

let editSeq = 0;

const stores = new WeakMap<UiStore, StoreApi<PluginsState>>();

export function createPluginsStore(): StoreApi<PluginsState> {
  return createStore<PluginsState>()((set, get) => ({
    dialogs: [],
    toasts: [],
    installMode: "package",
    edits: [],
    recent: [],
    veto: null,
    openDialog: (dialog) => {
      set((s) => {
        const index = s.dialogs.findIndex((d) => d.dialog_id === dialog.dialog_id);
        if (index < 0) return { dialogs: [...s.dialogs, dialog] };
        const dialogs = [...s.dialogs];
        dialogs[index] = dialog;
        return { dialogs };
      });
    },
    closeDialog: (dialogId) => {
      set((s) => ({ dialogs: s.dialogs.filter((d) => d.dialog_id !== dialogId) }));
    },
    closePluginDialogs: (pluginId) => {
      set((s) => ({ dialogs: s.dialogs.filter((d) => d.plugin_id !== pluginId) }));
    },
    pushToast: (toast) => {
      set((s) => ({
        toasts: [...s.toasts.filter((t) => t.id !== toast.id), toast].slice(-MAX_TOASTS),
      }));
    },
    dismissToast: (id) => {
      set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
    },
    setInstallMode: (installMode) => {
      set({ installMode });
    },
    pushEdit: (edit) => {
      set((s) => ({
        edits: [...s.edits, { ...edit, seq: ++editSeq }].slice(-MAX_PENDING_EDITS),
      }));
    },
    takeEdits: (draftId) => {
      const taken = get().edits.filter((e) => e.draftId === draftId);
      if (taken.length > 0) {
        set((s) => ({
          edits: s.edits.filter((e) => e.draftId !== draftId),
          recent: [...s.recent, ...taken].slice(-MAX_RECENT_EDITS),
        }));
      }
      return taken;
    },
    showVeto: (veto) => {
      const previous = get().veto;
      set({ veto });
      if (previous && previous !== veto) previous.settle(null);
    },
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
