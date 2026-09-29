/**
 * Client-side UI state (Zustand): selection, pane visibility, split sizes,
 * active tabs, keyboard context, connection and sync status. Server data
 * lives in TanStack Query, not here. Each browser session has its own UI
 * state on shared data (design.md, "Remote UI").
 */
import { createContext, useContext } from "react";
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { ConnectionStatus } from "../api/client";
import type { AccountId, FolderId, MessageId, ThemeMode } from "../api/generated";

export interface SyncState {
  accountId: AccountId;
  done: number;
  total: number;
}

export interface ConfigErrorState {
  file: string;
  line: number;
  column: number;
  message: string;
}

export interface UiState {
  activeAccountId: AccountId | null;
  activeFolderId: FolderId | null;
  selectedMessageId: MessageId | null;
  /** Ids of the messages currently loaded in the list, in display order. */
  messageOrder: MessageId[];
  /** Layout node ids hidden via `view.toggle`. */
  hiddenNodes: string[];
  /** User-dragged split sizes by split key, one CSS size per child. */
  splitSizes: Record<string, string[]>;
  /** Active tab index by tabs key. */
  activeTabs: Record<string, number>;
  /** Keyboard context last focused (`list`, `reader`, …). */
  keyContext: string;
  connection: ConnectionStatus;
  sync: SyncState | null;
  configError: ConfigErrorState | null;
  /** User override of the theme mode. */
  modeOverride: ThemeMode | null;
  /** User override of the UI language. */
  localeOverride: string | null;

  selectAccount(id: AccountId): void;
  selectFolder(id: FolderId | null): void;
  selectMessage(id: MessageId | null): void;
  setMessageOrder(ids: MessageId[]): void;
  toggleNode(id: string): void;
  setSplitSizes(key: string, sizes: string[]): void;
  setActiveTab(key: string, index: number): void;
  setKeyContext(context: string): void;
  setConnection(status: ConnectionStatus): void;
  setSync(sync: SyncState | null): void;
  setConfigError(error: ConfigErrorState | null): void;
  setModeOverride(mode: ThemeMode | null): void;
  setLocaleOverride(locale: string | null): void;
}

export type UiStore = StoreApi<UiState>;

export function createUiStore(initial: Partial<UiState> = {}): UiStore {
  return createStore<UiState>()((set) => ({
    activeAccountId: null,
    activeFolderId: null,
    selectedMessageId: null,
    messageOrder: [],
    hiddenNodes: [],
    splitSizes: {},
    activeTabs: {},
    keyContext: "list",
    connection: "connecting",
    sync: null,
    configError: null,
    modeOverride: null,
    localeOverride: null,

    selectAccount: (id) =>
      set((s) =>
        s.activeAccountId === id
          ? s
          : {
              activeAccountId: id,
              activeFolderId: null,
              selectedMessageId: null,
              messageOrder: [],
            },
      ),
    selectFolder: (id) =>
      set((s) =>
        s.activeFolderId === id
          ? s
          : { activeFolderId: id, selectedMessageId: null, messageOrder: [] },
      ),
    selectMessage: (id) => set({ selectedMessageId: id }),
    setMessageOrder: (ids) => set({ messageOrder: ids }),
    toggleNode: (id) =>
      set((s) => ({
        hiddenNodes: s.hiddenNodes.includes(id)
          ? s.hiddenNodes.filter((n) => n !== id)
          : [...s.hiddenNodes, id],
      })),
    setSplitSizes: (key, sizes) => set((s) => ({ splitSizes: { ...s.splitSizes, [key]: sizes } })),
    setActiveTab: (key, index) => set((s) => ({ activeTabs: { ...s.activeTabs, [key]: index } })),
    setKeyContext: (context) =>
      set((s) => (s.keyContext === context ? s : { keyContext: context })),
    setConnection: (connection) => set({ connection }),
    setSync: (sync) => set({ sync }),
    setConfigError: (configError) => set({ configError }),
    setModeOverride: (modeOverride) => set({ modeOverride }),
    setLocaleOverride: (localeOverride) => set({ localeOverride }),
    ...initial,
  }));
}

export const UiStoreContext = createContext<UiStore | null>(null);

/** The store instance of the surrounding app. */
export function useUiStoreApi(): UiStore {
  const store = useContext(UiStoreContext);
  if (!store) {
    throw new Error("UI store hooks must be used inside <UiStoreContext.Provider>");
  }
  return store;
}

/** Subscribes to a slice of the UI state. */
export function useUi<T>(selector: (state: UiState) => T): T {
  return useStore(useUiStoreApi(), selector);
}
