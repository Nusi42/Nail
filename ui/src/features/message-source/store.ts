/**
 * UI state of the source windows, one store per UI store: the message each
 * `message-source:<n>` window shows. `openMessageSource` raises the window
 * already showing a message instead of opening a second one.
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { MessageId } from "../../api/generated";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";
import { newWindowInstance } from "../../state/windows";
import { MESSAGE_SOURCE_KIND } from "./names";

export interface SourceUiState {
  /** Window id → the message it shows. */
  messages: Record<string, MessageId>;
  setMessage: (window: string, message: MessageId) => void;
}

const stores = new WeakMap<UiStore, StoreApi<SourceUiState>>();

/** The source store of a UI store (created on first use). */
export function sourceStoreFor(ui: UiStore): StoreApi<SourceUiState> {
  let store = stores.get(ui);
  if (!store) {
    store = createStore<SourceUiState>()((set) => ({
      messages: {},
      setMessage: (window, message) => {
        set((s) => ({ messages: { ...s.messages, [window]: message } }));
      },
    }));
    stores.set(ui, store);
  }
  return store;
}

export function useSourceState<T>(selector: (state: SourceUiState) => T): T {
  return useStore(sourceStoreFor(useUiStoreApi()), selector);
}

/** Opens (or raises) the source window of `message`; returns its id. */
export function openMessageSource(ui: UiStore, message: MessageId): string {
  const { windows } = ui.getState();
  const shown = sourceStoreFor(ui).getState().messages;
  const id =
    windows.find((w) => shown[w] === message) ?? newWindowInstance(MESSAGE_SOURCE_KIND, windows);
  sourceStoreFor(ui).getState().setMessage(id, message);
  ui.getState().openWindow(id);
  return id;
}
