/**
 * UI state of the rules feature: which rule is being edited, a rule
 * pre-filled by "create rule from message" waiting for the editor, and the
 * rules checked in the list to run together and the outcome of a run from
 * a menu (R1). One
 * store per client (so every app instance, and every test, has its own).
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { NailClient } from "../../api/client";
import type { FolderId, MessageId, Rule, RuleId, RuleRun } from "../../api/generated";
import { useClient } from "../../state/queries";

/**
 * The rule being edited: a saved rule, or a new one. `key` identifies the
 * editor instance: a new rule keeps its editor (and its messages) when it
 * is saved.
 */
export type Editing =
  { kind: "saved"; id: RuleId; key: string } | { kind: "new"; rule: Rule; key: string };

/**
 * What a run from a menu did (R1), shown by `RunToast`: the runs of one
 * click (undone together), their undo, or the core's error (Fluent id).
 */
export type RunToast =
  | { kind: "ran"; seq: number; runs: readonly RuleRun[] }
  | { kind: "undone"; seq: number }
  | { kind: "error"; seq: number; id: string };

/**
 * The message the rules window was opened from (R2, ADR 0222): its values
 * are offered next to condition fields (`./fromMessage`) and its folder is
 * the default "Run on" target.
 */
export interface RulesOrigin {
  messageId: MessageId;
  folderId: FolderId;
}

type DistributiveOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

export interface RulesUiState {
  editing: Editing | null;
  /** Edits a saved rule. */
  select: (id: RuleId | null) => void;
  /** The rule being edited was saved as `id` (a new rule becomes a saved one). */
  saved: (id: RuleId) => void;
  /** Starts a new rule from `rule` (a blank rule or one from a message). */
  startNew: (rule: Rule) => void;
  /** The message the window was opened from, `null` when opened otherwise. */
  origin: RulesOrigin | null;
  setOrigin: (origin: RulesOrigin | null) => void;
  /** Rules checked in the list to run together (R1), in the order checked. */
  picked: readonly RuleId[];
  /** Checks or unchecks a rule for running. */
  pick: (id: RuleId, on: boolean) => void;
  /** Keeps only the checked rules that still exist (after a delete). */
  prunePicked: (existing: readonly RuleId[]) => void;
  /** The outcome of the last run from a menu, `null` when dismissed. */
  toast: RunToast | null;
  showToast: (toast: DistributiveOmit<RunToast, "seq">) => void;
  dismissToast: () => void;
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
      origin: null,
      setOrigin: (origin) => {
        set({ origin });
      },
      picked: [],
      pick: (id, on) => {
        set((s) => {
          const rest = s.picked.filter((p) => p !== id);
          return { picked: on ? [...rest, id] : rest };
        });
      },
      prunePicked: (existing) => {
        set((s) => {
          const kept = s.picked.filter((p) => existing.includes(p));
          return kept.length === s.picked.length ? s : { picked: kept };
        });
      },
      toast: null,
      showToast: (toast) => {
        set({ toast: { ...toast, seq: ++seq } });
      },
      dismissToast: () => {
        set({ toast: null });
      },
    }));
    stores.set(client, store);
  }
  return store;
}

export function useRulesUi<T>(selector: (state: RulesUiState) => T): T {
  return useStore(rulesStore(useClient()), selector);
}
