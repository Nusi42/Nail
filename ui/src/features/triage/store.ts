/**
 * UI state of the triage feature, one store per UI store: the messages the
 * snooze picker is about (set by `snooze.open` before the dialog opens).
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { MessageId } from "../../api/generated";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";

export interface TriageState {
  /** Messages the snooze dialog snoozes. */
  snoozing: MessageId[];
  setSnoozing: (ids: MessageId[]) => void;
}

const stores = new WeakMap<UiStore, StoreApi<TriageState>>();

export function triageStoreFor(ui: UiStore): StoreApi<TriageState> {
  const existing = stores.get(ui);
  if (existing) return existing;
  const store = createStore<TriageState>()((set) => ({
    snoozing: [],
    setSnoozing: (snoozing) => {
      set({ snoozing });
    },
  }));
  stores.set(ui, store);
  return store;
}

export function useTriage<T>(selector: (state: TriageState) => T): T {
  return useStore(triageStoreFor(useUiStoreApi()), selector);
}
