/**
 * UI state of the triage feature, one store per UI store: the messages the
 * snooze picker is about (set by `snooze.open` before the dialog opens).
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { ActionTargets } from "../../actions/mailTargets";
import type { MessageId } from "../../api/generated";
import { useUiStoreApi, type UiStore, UiStoreMap } from "../../state/uiStore";

export interface TriageState {
  /** Messages the snooze dialog snoozes (when it has no `targets`). */
  snoozing: MessageId[];
  /**
   * What the snooze dialog snoozes (P3): the selected conversation (its
   * messages in the listed folder), the picked rows or a card's message.
   */
  targets: ActionTargets | null;
  setSnoozing: (ids: MessageId[]) => void;
  setSnoozeTargets: (targets: ActionTargets) => void;
}

const stores = new UiStoreMap<StoreApi<TriageState>>();

export function triageStoreFor(ui: UiStore): StoreApi<TriageState> {
  return stores.obtain(ui, () => {
    const store = createStore<TriageState>()((set) => ({
      snoozing: [],
      targets: null,
      setSnoozing: (snoozing) => {
        set({ snoozing, targets: null });
      },
      setSnoozeTargets: (targets) => {
        set({
          targets,
          snoozing: targets.targets.flatMap((t) =>
            t.kind === "message" ? [t.message_id] : t.kind === "conversation" ? [t.thread_id] : [],
          ),
        });
      },
    }));
    return store;
  });
}

export function useTriage<T>(selector: (state: TriageState) => T): T {
  return useStore(triageStoreFor(useUiStoreApi()), selector);
}
