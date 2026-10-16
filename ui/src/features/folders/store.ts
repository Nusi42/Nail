/**
 * What the folder windows work on, one store per UI store (ADR 0196).
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";

export type FolderTask =
  | { kind: "create"; accountId: string; parentId: string | null }
  | { kind: "rename"; accountId: string; folderId: string }
  | { kind: "move"; accountId: string; folderId: string }
  | { kind: "delete"; accountId: string; folderId: string };

export interface FoldersUiState {
  task: FolderTask | null;
  setTask: (task: FolderTask | null) => void;
}

const stores = new WeakMap<UiStore, StoreApi<FoldersUiState>>();

export function foldersStoreFor(ui: UiStore): StoreApi<FoldersUiState> {
  let store = stores.get(ui);
  if (!store) {
    store = createStore<FoldersUiState>()((set) => ({
      task: null,
      setTask: (task) => {
        set({ task });
      },
    }));
    stores.set(ui, store);
  }
  return store;
}

export function useFolderTask(): FolderTask | null {
  return useStore(foldersStoreFor(useUiStoreApi()), (s) => s.task);
}
