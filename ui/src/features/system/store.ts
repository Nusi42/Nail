/**
 * UI state of this feature, one store per UI store: which drafts another
 * program prepared (Simple MAPI "Send to"; shown as a notice in the
 * composer) and whether the first-run screen was offered in this session.
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { DraftId } from "../../api/generated";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";

export interface SystemUiState {
  /** Drafts from other programs → the program's file name (`null` = unknown). */
  programDrafts: Record<DraftId, string | null>;
  /** The first-run screen was opened once in this session. */
  firstRunOffered: boolean;
  noteProgramDraft(draftId: DraftId, program: string | null): void;
  /** The user dismissed the notice (or the draft was sent / discarded). */
  forgetProgramDraft(draftId: DraftId): void;
  setFirstRunOffered(): void;
}

const stores = new WeakMap<UiStore, StoreApi<SystemUiState>>();

export function createSystemStore(): StoreApi<SystemUiState> {
  return createStore<SystemUiState>()((set) => ({
    programDrafts: {},
    firstRunOffered: false,
    noteProgramDraft: (draftId, program) =>
      set((s) => ({ programDrafts: { ...s.programDrafts, [draftId]: program } })),
    forgetProgramDraft: (draftId) =>
      set((s) => ({
        programDrafts: Object.fromEntries(
          Object.entries(s.programDrafts).filter(([id]) => id !== draftId),
        ),
      })),
    setFirstRunOffered: () => set({ firstRunOffered: true }),
  }));
}

/** The system store of a UI store. */
export function systemStoreFor(ui: UiStore): StoreApi<SystemUiState> {
  let store = stores.get(ui);
  if (!store) {
    store = createSystemStore();
    stores.set(ui, store);
  }
  return store;
}

export function useSystemState<T>(selector: (state: SystemUiState) => T): T {
  return useStore(systemStoreFor(useUiStoreApi()), selector);
}
