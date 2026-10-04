/**
 * UI state of printing, one store per UI store: what the print dialog was
 * opened for (target, Print or Save as PDF), the print job whose view
 * should open the system print dialog once it is laid out, and a local
 * sheet (content the UI prints without a core job, e.g. the recovery kit).
 */
import type { ReactNode } from "react";
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { PrintJobId, PrintTarget } from "../../api/generated";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";

export type PrintMode = "print" | "pdf";

export interface PrintUiState {
  /** What the print dialog prints (`null` = nothing chosen). */
  target: PrintTarget | null;
  /** Whether the dialog was opened to print or to save a PDF. */
  mode: PrintMode;
  /** The job whose print view opens the system print dialog when ready. */
  autoPrint: PrintJobId | null;
  /** Content printed without a job (shown by the print host, then printed). */
  sheet: ReactNode | null;
  prepare: (target: PrintTarget | null, mode: PrintMode) => void;
  setAutoPrint: (jobId: PrintJobId | null) => void;
  setSheet: (sheet: ReactNode | null) => void;
}

const stores = new WeakMap<UiStore, StoreApi<PrintUiState>>();

export function createPrintStore(): StoreApi<PrintUiState> {
  return createStore<PrintUiState>()((set) => ({
    target: null,
    mode: "print",
    autoPrint: null,
    sheet: null,
    prepare: (target, mode) => {
      set({ target, mode });
    },
    setAutoPrint: (autoPrint) => {
      set({ autoPrint });
    },
    setSheet: (sheet) => {
      set({ sheet });
    },
  }));
}

/** The print store of a UI store. */
export function printStoreFor(ui: UiStore): StoreApi<PrintUiState> {
  let store = stores.get(ui);
  if (!store) {
    store = createPrintStore();
    stores.set(ui, store);
  }
  return store;
}

export function usePrintState<T>(selector: (state: PrintUiState) => T): T {
  return useStore(printStoreFor(useUiStoreApi()), selector);
}
