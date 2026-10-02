/**
 * UI state of this feature, one store per UI store: which finished job the
 * report dialog shows, whether the system file dialog is unavailable (web
 * mode: paths are typed), and what the diagnostics dialog should include
 * when it was opened for particular messages.
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { JobId, MessageId } from "../../api/generated";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";

export interface PortabilityUiState {
  /** The job the report dialog shows. */
  reportJobId: JobId | null;
  /** `PickPath` answered `unsupported`: no Browse buttons, typed paths only. */
  pickerUnavailable: boolean;
  /** Messages the diagnostics dialog offers to include (opt-in). */
  diagnosticsMessages: MessageId[];
  showReport(jobId: JobId): void;
  setPickerUnavailable(): void;
  setDiagnosticsMessages(ids: MessageId[]): void;
}

const stores = new WeakMap<UiStore, StoreApi<PortabilityUiState>>();

export function createPortabilityStore(): StoreApi<PortabilityUiState> {
  return createStore<PortabilityUiState>()((set) => ({
    reportJobId: null,
    pickerUnavailable: false,
    diagnosticsMessages: [],
    showReport: (reportJobId) => set({ reportJobId }),
    setPickerUnavailable: () => set({ pickerUnavailable: true }),
    setDiagnosticsMessages: (diagnosticsMessages) => set({ diagnosticsMessages }),
  }));
}

/** The portability store of a UI store. */
export function portabilityStoreFor(ui: UiStore): StoreApi<PortabilityUiState> {
  let store = stores.get(ui);
  if (!store) {
    store = createPortabilityStore();
    stores.set(ui, store);
  }
  return store;
}

export function usePortabilityState<T>(selector: (state: PortabilityUiState) => T): T {
  return useStore(portabilityStoreFor(useUiStoreApi()), selector);
}
