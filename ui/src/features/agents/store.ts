/**
 * UI state of the confirmation center, one store per UI store: the
 * notices ("something waits") shown as toasts, and which confirmation the
 * center should bring into view when it opens.
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { Confirmation, ConfirmationId } from "../../api/generated";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";

/** Most notices on screen at once (older ones make room). */
export const MAX_NOTICES = 3;

export interface AgentsUiState {
  /** Pending confirmations announced as toasts, oldest first. */
  notices: Confirmation[];
  /** The confirmation to bring into view in the center (`null` = none). */
  focus: ConfirmationId | null;
  /** A confirmation was requested: show a notice. */
  notice(confirmation: Confirmation): void;
  /** A notice was dismissed, or its confirmation decided. */
  dismiss(id: ConfirmationId): void;
  setFocus(id: ConfirmationId | null): void;
}

const stores = new WeakMap<UiStore, StoreApi<AgentsUiState>>();

export function createAgentsStore(): StoreApi<AgentsUiState> {
  return createStore<AgentsUiState>()((set) => ({
    notices: [],
    focus: null,
    notice: (confirmation) =>
      set((s) => ({
        notices: [...s.notices.filter((n) => n.id !== confirmation.id), confirmation].slice(
          -MAX_NOTICES,
        ),
      })),
    dismiss: (id) => set((s) => ({ notices: s.notices.filter((n) => n.id !== id) })),
    setFocus: (focus) => set({ focus }),
  }));
}

/** The agents store of a UI store. */
export function agentsStoreFor(ui: UiStore): StoreApi<AgentsUiState> {
  let store = stores.get(ui);
  if (!store) {
    store = createAgentsStore();
    stores.set(ui, store);
  }
  return store;
}

export function useAgentsState<T>(selector: (state: AgentsUiState) => T): T {
  return useStore(agentsStoreFor(useUiStoreApi()), selector);
}
