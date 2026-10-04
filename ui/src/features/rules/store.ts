/**
 * UI state of the rules feature: which rule is being edited, and a rule
 * pre-filled by "create rule from message" waiting for the editor. One
 * store per client (so every app instance, and every test, has its own).
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { NailClient } from "../../api/client";
import type { Rule, RuleId } from "../../api/generated";
import { useClient } from "../../state/queries";

/**
 * The rule being edited: a saved rule, or a new one. `key` identifies the
 * editor instance: a new rule keeps its editor (and its messages) when it
 * is saved.
 */
export type Editing =
  { kind: "saved"; id: RuleId; key: string } | { kind: "new"; rule: Rule; key: string };

export interface RulesUiState {
  editing: Editing | null;
  /** Edits a saved rule. */
  select: (id: RuleId | null) => void;
  /** The rule being edited was saved as `id` (a new rule becomes a saved one). */
  saved: (id: RuleId) => void;
  /** Starts a new rule from `rule` (a blank rule or one from a message). */
  startNew: (rule: Rule) => void;
}

const stores = new WeakMap<NailClient, StoreApi<RulesUiState>>();

export function rulesStore(client: NailClient): StoreApi<RulesUiState> {
  let store = stores.get(client);
  if (!store) {
    let seq = 0;
    store = createStore<RulesUiState>((set) => ({
      editing: null,
      select: (id) => {
        set({ editing: id === null ? null : { kind: "saved", id, key: `saved-${id}` } });
      },
      saved: (id) => {
        set((s) => ({ editing: { kind: "saved", id, key: s.editing?.key ?? `saved-${id}` } }));
      },
      startNew: (rule) => {
        set({ editing: { kind: "new", rule, key: `new-${String(++seq)}` } });
      },
    }));
    stores.set(client, store);
  }
  return store;
}

export function useRulesUi<T>(selector: (state: RulesUiState) => T): T {
  return useStore(rulesStore(useClient()), selector);
}
