/**
 * UI state of the settings page, one store per UI store: the search text,
 * a request to focus the search (`settings.search`), and the setting a deep
 * link or a search result asked to show (scrolled to and focused once the
 * section rendered). The shown section itself is the core's UI state
 * (`settings.section`), so it is remembered across restarts.
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";

/** A setting to reveal in the shown section. */
export interface RevealRequest {
  section: string;
  /** The widget node (`data-node-id`) to reveal, if known. */
  node: string | null;
  /** The widget kind to reveal (the first node of that kind), if known. */
  widget: string | null;
  /** The setting's label as shown, to find the row inside the widget. */
  text: string | null;
  /** Grows with every request, so the same target can be revealed again. */
  seq: number;
}

export interface SettingsUiState {
  query: string;
  /** Grows with every `settings.search`; the page focuses its search box. */
  focusSearch: number;
  reveal: RevealRequest | null;
  setQuery: (query: string) => void;
  requestSearchFocus: () => void;
  requestReveal: (request: Omit<RevealRequest, "seq">) => void;
  /** Forgets a handled request (only the one with `seq`). */
  revealed: (seq: number) => void;
}

const stores = new WeakMap<UiStore, StoreApi<SettingsUiState>>();

export function createSettingsStore(): StoreApi<SettingsUiState> {
  return createStore<SettingsUiState>()((set) => ({
    query: "",
    focusSearch: 0,
    reveal: null,
    setQuery: (query) => {
      set({ query });
    },
    requestSearchFocus: () => {
      set((s) => ({ focusSearch: s.focusSearch + 1 }));
    },
    requestReveal: (request) => {
      set((s) => ({ reveal: { ...request, seq: (s.reveal?.seq ?? 0) + 1 } }));
    },
    revealed: (seq) => {
      set((s) => (s.reveal?.seq === seq ? { reveal: null } : {}));
    },
  }));
}

/** The settings page store of a UI store. */
export function settingsStoreFor(ui: UiStore): StoreApi<SettingsUiState> {
  let store = stores.get(ui);
  if (!store) {
    store = createSettingsStore();
    stores.set(ui, store);
  }
  return store;
}

export function useSettingsState<T>(selector: (state: SettingsUiState) => T): T {
  return useStore(settingsStoreFor(useUiStoreApi()), selector);
}
