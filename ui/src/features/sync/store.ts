/**
 * UI state of the sync feature, one store per UI store: the folder the
 * `sync-folder` dialog edits, and bodies being downloaded on demand.
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { FolderId, MessageId } from "../../api/generated";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";

export interface SyncState {
  /** The folder of the `sync-folder` dialog. */
  folderId: FolderId | null;
  /** Messages whose body is being downloaded. */
  fetching: MessageId[];
  /** Messages whose last download failed. */
  failed: MessageId[];
  setFolder: (id: FolderId | null) => void;
  started: (ids: readonly MessageId[]) => void;
  finished: (fetched: readonly MessageId[], failed: readonly MessageId[]) => void;
}

const stores = new WeakMap<UiStore, StoreApi<SyncState>>();

export function syncStoreFor(ui: UiStore): StoreApi<SyncState> {
  const existing = stores.get(ui);
  if (existing) return existing;
  const store = createStore<SyncState>()((set) => ({
    folderId: null,
    fetching: [],
    failed: [],
    setFolder: (folderId) => {
      set({ folderId });
    },
    started: (ids) => {
      set((s) => ({
        fetching: [...new Set([...s.fetching, ...ids])],
        failed: s.failed.filter((id) => !ids.includes(id)),
      }));
    },
    finished: (fetched, failed) => {
      set((s) => ({
        fetching: s.fetching.filter((id) => !fetched.includes(id) && !failed.includes(id)),
        failed: [...new Set([...s.failed.filter((id) => !fetched.includes(id)), ...failed])],
      }));
    },
  }));
  stores.set(ui, store);
  return store;
}

export function useSync<T>(selector: (state: SyncState) => T): T {
  return useStore(syncStoreFor(useUiStoreApi()), selector);
}
