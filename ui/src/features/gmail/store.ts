/**
 * UI state of the Gmail feature (one store per UI store): what the label
 * editor and the delete confirmation work on.
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { AccountId, GmailLabel, GmailLabelId } from "../../api/generated";
import { useUiStoreApi, type UiStore, UiStoreMap } from "../../state/uiStore";

/** What the label editor edits. */
export interface LabelEditorTarget {
  accountId: AccountId;
  /** `null` = a new label. */
  labelId: GmailLabelId | null;
  /** Parent of a new label (`Work` for a new `Work/…`). */
  parent: string | null;
}

export interface GmailState {
  editor: LabelEditorTarget | null;
  deleting: GmailLabel | null;
  editLabel: (target: LabelEditorTarget | null) => void;
  deleteLabel: (label: GmailLabel | null) => void;
}

const stores = new UiStoreMap<StoreApi<GmailState>>();

export function gmailStoreFor(ui: UiStore): StoreApi<GmailState> {
  return stores.obtain(ui, () => {
    const store = createStore<GmailState>()((set) => ({
      editor: null,
      deleting: null,
      editLabel: (editor) => {
        set({ editor });
      },
      deleteLabel: (deleting) => {
        set({ deleting });
      },
    }));
    return store;
  });
}

export function useGmail<T>(selector: (state: GmailState) => T): T {
  return useStore(gmailStoreFor(useUiStoreApi()), selector);
}
