/**
 * UI state of the palette, one store per UI store: the contexts captured
 * when it opened (focus moves into the palette, so they cannot be read
 * afterwards) and the action it last ran.
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { ActionAvailability } from "../../api/generated";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";

export interface PaletteUiState {
  /** Contexts when the palette opened; `null` = read them now. */
  contexts: ActionAvailability[] | null;
  /** The last action run from the palette (tests, telemetry-free). */
  lastRun: string | null;
  setContexts: (contexts: ActionAvailability[] | null) => void;
  setLastRun: (id: string) => void;
}

const stores = new WeakMap<UiStore, StoreApi<PaletteUiState>>();

export function createPaletteStore(): StoreApi<PaletteUiState> {
  return createStore<PaletteUiState>()((set) => ({
    contexts: null,
    lastRun: null,
    setContexts: (contexts) => {
      set({ contexts });
    },
    setLastRun: (lastRun) => {
      set({ lastRun });
    },
  }));
}

/** The palette store of a UI store. */
export function paletteStoreFor(ui: UiStore): StoreApi<PaletteUiState> {
  let store = stores.get(ui);
  if (!store) {
    store = createPaletteStore();
    stores.set(ui, store);
  }
  return store;
}

export function usePaletteState<T>(selector: (state: PaletteUiState) => T): T {
  return useStore(paletteStoreFor(useUiStoreApi()), selector);
}
