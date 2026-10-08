/**
 * UI state of the connection log feature, one store per UI store: which
 * scope each viewer window shows (and the entry to point at), and the
 * error each details window explains. Opening functions for other slots
 * (status bar, account settings).
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type {
  AccountId,
  ConnectionLogScope,
  ErrorCode,
  ErrorDetails,
  LocalizedMessage,
} from "../../api/generated";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";
import { newWindowInstance } from "../../state/windows";
import { scopeOfViewerWindow, viewerWindowOf } from "./model";
import { CONNECTION_LOG_KIND, ERROR_DETAILS_KIND } from "./names";

/** A viewer window's scope and the position to point at. */
export interface ViewerTarget {
  scope: ConnectionLogScope;
  /** `ErrorDetails.log_seq` of the error that opened it. */
  highlight: number | null;
}

/** What a details window explains. */
export interface ErrorReport {
  /** The account the error belongs to (its connection log). */
  accountId: AccountId | null;
  /** The error class (the wrapper text when there is no `localized`). */
  code: ErrorCode;
  /** The core's user-facing text, if it sent one. */
  localized: LocalizedMessage | null;
  /** The precise cause. */
  details: ErrorDetails;
}

export interface ConnlogUiState {
  viewers: Record<string, ViewerTarget>;
  reports: Record<string, ErrorReport>;
  setViewer: (window: string, target: ViewerTarget) => void;
  setReport: (window: string, report: ErrorReport) => void;
}

const stores = new WeakMap<UiStore, StoreApi<ConnlogUiState>>();

function createConnlogStore(): StoreApi<ConnlogUiState> {
  return createStore<ConnlogUiState>()((set) => ({
    viewers: {},
    reports: {},
    setViewer: (window, target) => {
      set((s) => ({ viewers: { ...s.viewers, [window]: target } }));
    },
    setReport: (window, report) => {
      set((s) => ({ reports: { ...s.reports, [window]: report } }));
    },
  }));
}

/** The connection log store of a UI store. */
export function connlogStoreFor(ui: UiStore): StoreApi<ConnlogUiState> {
  let store = stores.get(ui);
  if (!store) {
    store = createConnlogStore();
    stores.set(ui, store);
  }
  return store;
}

export function useConnlogState<T>(selector: (state: ConnlogUiState) => T): T {
  return useStore(connlogStoreFor(useUiStoreApi()), selector);
}

/** Opens (or raises) the viewer of `scope`, pointing at `highlight`. */
export function openConnectionLog(
  ui: UiStore,
  scope: ConnectionLogScope,
  highlight: number | null = null,
): string {
  const id = viewerWindowOf(scope) ?? newWindowInstance(CONNECTION_LOG_KIND, ui.getState().windows);
  connlogStoreFor(ui).getState().setViewer(id, { scope, highlight });
  ui.getState().openWindow(id);
  return id;
}

/** The scope a viewer window shows (from the store, else its id). */
export function viewerTarget(ui: UiStore, window: string): ViewerTarget | null {
  const stored = connlogStoreFor(ui).getState().viewers[window];
  if (stored) return stored;
  const scope = scopeOfViewerWindow(window);
  return scope ? { scope, highlight: null } : null;
}

/** Opens a new details window for `report`. */
export function openErrorDetails(ui: UiStore, report: ErrorReport): string {
  const id = newWindowInstance(ERROR_DETAILS_KIND, ui.getState().windows);
  connlogStoreFor(ui).getState().setReport(id, report);
  ui.getState().openWindow(id);
  return id;
}
