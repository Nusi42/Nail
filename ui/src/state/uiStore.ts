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
import type {
  AccountId,
  DraftContext,
  DraftId,
  ErrorCode,
  ErrorDetails,
  FolderId,
  ListFilter,
  LocalizedMessage,
  IdentityId,
  MessageId,
  OpId,
  OutboxEntry,
  SearchError,
  ThemeMode,
  ThreadId,
} from "../api/generated";

export interface SyncState {
  accountId: AccountId;
  done: number;
  total: number;
}

export interface SyncErrorState {
  accountId: AccountId;
  code: ErrorCode;
  /** The core's text for the failure (Fluent id + arguments), if it sent one. */
  localized?: LocalizedMessage | null;
  /** The precise cause, for "Show details" (ADR 0190). */
  details?: ErrorDetails | null;
}

export interface ConfigErrorState {
  file: string;
  line: number;
  column: number;
  message: string;
}

/** The message being written in the composer (plain text, M1a). */
export interface ComposerState {
  /** Saved draft id, `null` until the first save. */
  draftId: DraftId | null;
  identityId: IdentityId | null;
  /** Recipient fields as typed (comma-separated addresses). */
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  text: string;
  /** The message replied to or forwarded (`null` for a new message). */
  context: DraftContext | null;
  showCc: boolean;
  showBcc: boolean;
  /** `true` once the user picked an identity by hand (rules stop applying). */
  identityPicked: boolean;
  busy: "saving" | "sending" | null;
  /** Fluent id of the last problem, with its arguments. */
  error: { id: string; args?: Record<string, string | number> } | null;
}

/** An active search (the message list shows its results). */
export interface SearchState {
  text: string;
  /** Scope chosen when the search started (`null` = everywhere). */
  accountId: AccountId | null;
  folderId: FolderId | null;
  /** Set when the core rejected the query. */
  error: SearchError | null;
}

/** A focus request for a widget (`search.focus`), identified by a counter. */
export interface FocusRequest {
  target: string;
  seq: number;
}

export interface UiState {
  activeAccountId: AccountId | null;
  activeFolderId: FolderId | null;
  selectedMessageId: MessageId | null;
  /** Ids of the messages currently loaded in the list, in display order. */
  messageOrder: MessageId[];
  /**
   * The conversation each listed message's row shows (P2): j / k step on
   * from a conversation's row while the reader selected one of its messages
   * that has no row (an older card, the user's reply from Sent).
   */
  messageThreads: Readonly<Record<MessageId, ThreadId>>;
  /** Layout node ids hidden via `view.toggle`. */
  hiddenNodes: string[];
  /** User-dragged split sizes by split key, one CSS size per child. */
  splitSizes: Record<string, string[]>;
  /** Active tab index by tabs key. */
  activeTabs: Record<string, number>;
  /**
   * The latest request to show the pane holding one of these widget kinds:
   * a tab set whose tab contains one switches to that tab (a narrow layout
   * shows the reader after Open). `seq` makes a repeated request count.
   */
  revealRequest: { kinds: readonly string[]; seq: number } | null;
  /** Keyboard context last focused (`list`, `reader`, …). */
  keyContext: string;
  connection: ConnectionStatus;
  sync: SyncState | null;
  /** The last failed synchronization, cleared when a new one reports progress. */
  syncError: SyncErrorState | null;
  configError: ConfigErrorState | null;
  /** User override of the theme mode. */
  modeOverride: ThemeMode | null;
  /** User override of the UI language. */
  localeOverride: string | null;
  /** The thread of the selected message, when known. */
  selectedThreadId: ThreadId | null;
  /**
   * Secondary windows open over the main window, in z-order: topmost (the
   * focused one) last. Entries are window ids (M5c, ADR 0116): a layout
   * window or dialog name, or `<kind>:<instance>` for one of several
   * windows of a kind (`./windows.ts`).
   */
  windows: string[];
  /**
   * The focused composer (M5c, ADR 0118). While `composer:<key>` windows
   * are open it mirrors the topmost one (`features/composer/sessions.ts`):
   * a write goes to that composer and `null` closes it. Without instance
   * windows it is the plain `composer` window's own state.
   */
  composer: ComposerState | null;
  /**
   * Composers of the windows `composer:<instance>` by window id (M5c, ADRs
   * 0116 and 0118; every new message opens one).
   */
  composers: Record<string, ComposerState>;
  search: SearchState | null;
  /** What is typed in the search bar (not yet necessarily searched). */
  searchInput: string;
  focusRequest: FocusRequest | null;
  /** Queued writes not yet replayed. */
  pendingOps: OpId[];
  /** The last failed write, until the next one succeeds or it is dismissed. */
  opError: ErrorCode | null;
  /** The operation behind `opError` (for "Retry", `RetryOp`). */
  failedOp: OpId | null;
  /** The last message handed to the outbox (undo-send countdown). */
  lastSend: OutboxEntry | null;
  /** Why `lastSend` failed, for "Show details" (ADR 0190). */
  lastSendDetails: ErrorDetails | null;
  /** Upload progress of messages being sent, by outbox id (`SendProgress`). */
  sendProgress: Record<string, SendProgressState>;
  /** Results of OAuth sign-ins by flow id (`null` = success). */
  oauthResults: Record<string, ErrorCode | null>;
  /**
   * The view shown in the `view-host` (M5b, ADR 0109; a key of the layout's
   * `views`), `null` = the layout's start view.
   */
  activeView: string | null;
  /** The message list's filter chip (M5b; `list.filter`). */
  listFilter: ListFilter;

  selectAccount: (id: AccountId) => void;
  selectFolder: (id: FolderId | null) => void;
  selectMessage: (id: MessageId | null) => void;
  setMessageOrder: (ids: MessageId[], threads?: Readonly<Record<MessageId, ThreadId>>) => void;
  toggleNode: (id: string) => void;
  setSplitSizes: (key: string, sizes: string[]) => void;
  setActiveTab: (key: string, index: number) => void;
  revealWidgets: (kinds: readonly string[]) => void;
  setKeyContext: (context: string) => void;
  setConnection: (status: ConnectionStatus) => void;
  setSync: (sync: SyncState | null) => void;
  setSyncError: (error: SyncErrorState | null) => void;
  setConfigError: (error: ConfigErrorState | null) => void;
  setModeOverride: (mode: ThemeMode | null) => void;
  setLocaleOverride: (locale: string | null) => void;
  selectThread: (id: ThreadId | null) => void;
  /** Opens a window (id) on top, or raises it when open. */
  openWindow: (name: string) => void;
  /** Closes the named window, or the topmost one. */
  closeWindow: (name?: string) => void;
  /** Raises an open window to the top (focus order = z-order; M5c). */
  focusWindow: (name: string) => void;
  /** Sets (`null` removes) the composer of a `composer:<instance>` window (M5c). */
  setComposerOf: (windowId: string, composer: ComposerState | null) => void;
  /** Patches the composer of a `composer:<instance>` window (M5c). */
  updateComposerOf: (windowId: string, patch: Partial<ComposerState>) => void;
  setComposer: (composer: ComposerState | null) => void;
  updateComposer: (patch: Partial<ComposerState>) => void;
  setSearch: (search: SearchState | null) => void;
  setSearchInput: (text: string) => void;
  requestFocus: (target: string) => void;
  opQueued: (ids: OpId[]) => void;
  opFinished: (id: OpId, error: ErrorCode | null) => void;
  dismissOpError: () => void;
  setLastSend: (entry: OutboxEntry | null) => void;
  setLastSendDetails: (details: ErrorDetails | null) => void;
  setSendProgress: (outboxId: string, progress: SendProgressState | null) => void;
  setOAuthResult: (flowId: string, error: ErrorCode | null) => void;
  /** Shows a view (M5b; `view.show`). */
  setActiveView: (view: string | null) => void;
  /** Sets the message list's filter chip (M5b). */
  setListFilter: (filter: ListFilter) => void;
}

/** Upload progress of a message being sent (`SendProgress`). */
export interface SendProgressState {
  sentBytes: number;
  totalBytes: number;
}

/** Whole percent uploaded (0–100). */
export function sendPercent(progress: SendProgressState): number {
  if (progress.totalBytes <= 0) return 0;
  return Math.min(100, Math.floor((progress.sentBytes / progress.totalBytes) * 100));
}

export type UiStore = StoreApi<UiState>;

export function createUiStore(initial: Partial<UiState> = {}): UiStore {
  return createStore<UiState>()((set) => ({
    activeAccountId: null,
    activeFolderId: null,
    selectedMessageId: null,
    messageOrder: [],
    messageThreads: {},
    hiddenNodes: [],
    splitSizes: {},
    activeTabs: {},
    revealRequest: null,
    keyContext: "list",
    connection: "connecting",
    sync: null,
    syncError: null,
    configError: null,
    modeOverride: null,
    localeOverride: null,
    selectedThreadId: null,
    windows: [],
    composer: null,
    composers: {},
    search: null,
    searchInput: "",
    focusRequest: null,
    pendingOps: [],
    opError: null,
    failedOp: null,
    lastSend: null,
    lastSendDetails: null,
    sendProgress: {},
    oauthResults: {},
    activeView: null,
    listFilter: "all",

    selectAccount: (id) => {
      set((s) =>
        s.activeAccountId === id
          ? s
          : {
              activeAccountId: id,
              activeFolderId: null,
              selectedMessageId: null,
              selectedThreadId: null,
              messageOrder: [],
              search: null,
            },
      );
    },
    selectFolder: (id) => {
      set((s) =>
        s.activeFolderId === id
          ? s
          : {
              activeFolderId: id,
              selectedMessageId: null,
              selectedThreadId: null,
              messageOrder: [],
              search: null,
            },
      );
    },
    selectMessage: (id) => {
      set({ selectedMessageId: id });
    },
    setMessageOrder: (ids, threads = {}) => {
      set({ messageOrder: ids, messageThreads: threads });
    },
    toggleNode: (id) => {
      set((s) => ({
        hiddenNodes: s.hiddenNodes.includes(id)
          ? s.hiddenNodes.filter((n) => n !== id)
          : [...s.hiddenNodes, id],
      }));
    },
    setSplitSizes: (key, sizes) => {
      set((s) => ({ splitSizes: { ...s.splitSizes, [key]: sizes } }));
    },
    setActiveTab: (key, index) => {
      set((s) => ({ activeTabs: { ...s.activeTabs, [key]: index } }));
    },
    revealWidgets: (kinds) => {
      set((s) => ({ revealRequest: { kinds, seq: (s.revealRequest?.seq ?? 0) + 1 } }));
    },
    setKeyContext: (context) => {
      set((s) => (s.keyContext === context ? s : { keyContext: context }));
    },
    setConnection: (connection) => {
      set({ connection });
    },
    setSync: (sync) => {
      set({ sync });
    },
    setSyncError: (syncError) => {
      set({ syncError });
    },
    setConfigError: (configError) => {
      set({ configError });
    },
    setModeOverride: (modeOverride) => {
      set({ modeOverride });
    },
    setLocaleOverride: (localeOverride) => {
      set({ localeOverride });
    },
    selectThread: (selectedThreadId) => {
      set({ selectedThreadId });
    },
    openWindow: (name) => {
      set((s) => ({ windows: [...s.windows.filter((w) => w !== name), name] }));
    },
    closeWindow: (name) => {
      set((s) => ({
        windows: name === undefined ? s.windows.slice(0, -1) : s.windows.filter((w) => w !== name),
      }));
    },
    focusWindow: (name) => {
      set((s) =>
        !s.windows.includes(name) || s.windows[s.windows.length - 1] === name
          ? s
          : { windows: [...s.windows.filter((w) => w !== name), name] },
      );
    },
    setComposerOf: (windowId, composer) => {
      set((s) => {
        if (composer !== null) return { composers: { ...s.composers, [windowId]: composer } };
        if (!Object.hasOwn(s.composers, windowId)) return s;
        return {
          composers: Object.fromEntries(
            Object.entries(s.composers).filter(([id]) => id !== windowId),
          ),
        };
      });
    },
    updateComposerOf: (windowId, patch) => {
      set((s) => {
        const current = Object.hasOwn(s.composers, windowId) ? s.composers[windowId] : undefined;
        return current
          ? { composers: { ...s.composers, [windowId]: { ...current, ...patch } } }
          : s;
      });
    },
    setComposer: (composer) => {
      set({ composer });
    },
    updateComposer: (patch) => {
      set((s) => (s.composer ? { composer: { ...s.composer, ...patch } } : s));
    },
    setSearch: (search) => {
      set((s) =>
        search === null
          ? { search: null, searchInput: "" }
          : {
              search,
              // A new search starts without a selection.
              ...(s.search?.text === search.text
                ? {}
                : { selectedMessageId: null, selectedThreadId: null, messageOrder: [] }),
            },
      );
    },
    setSearchInput: (searchInput) => {
      set({ searchInput });
    },
    requestFocus: (target) => {
      set((s) => ({ focusRequest: { target, seq: (s.focusRequest?.seq ?? 0) + 1 } }));
    },
    opQueued: (ids) => {
      set((s) => ({ pendingOps: [...s.pendingOps, ...ids] }));
    },
    opFinished: (id, error) => {
      set((s) => {
        const cleared = s.pendingOps.includes(id);
        return {
          pendingOps: s.pendingOps.filter((p) => p !== id),
          opError: error ?? (cleared ? null : s.opError),
          failedOp: error !== null ? id : cleared ? null : s.failedOp,
        };
      });
    },
    dismissOpError: () => {
      set({ opError: null, failedOp: null });
    },
    setLastSend: (lastSend) => {
      // Details belong to the entry they explain.
      set((s) =>
        s.lastSend?.id === lastSend?.id ? { lastSend } : { lastSend, lastSendDetails: null },
      );
    },
    setLastSendDetails: (lastSendDetails) => {
      set({ lastSendDetails });
    },
    setSendProgress: (outboxId, progress) => {
      set((s) => {
        if (progress === null) {
          if (!(outboxId in s.sendProgress)) return s;
          return {
            sendProgress: Object.fromEntries(
              Object.entries(s.sendProgress).filter(([id]) => id !== outboxId),
            ),
          };
        }
        return { sendProgress: { ...s.sendProgress, [outboxId]: progress } };
      });
    },
    setOAuthResult: (flowId, error) => {
      set((s) => ({ oauthResults: { ...s.oauthResults, [flowId]: error } }));
    },
    setActiveView: (activeView) => {
      set((s) => (s.activeView === activeView ? s : { activeView }));
    },
    setListFilter: (listFilter) => {
      set((s) => (s.listFilter === listFilter ? s : { listFilter }));
    },
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
