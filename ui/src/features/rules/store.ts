/**
 * UI state of the rules feature: which rule is being edited, a rule
 * pre-filled by "create rule from message" waiting for the editor, the
 * message the window was opened from, and the outcome of a run from a menu
 * (R1). One store per client (so every app instance, and every test, has
 * its own).
 *
 * Unsaved changes (R2 design review, item 4): the editor reports whether
 * its rule differs from the saved one (`setDirty`). While it does, opening
 * another rule (`select`, `startNew`) or closing the window
 * (`./closeGuard.ts`) does not happen at once: it waits in `pending` until
 * the user discards the changes (`discard`) or keeps editing (`keepEditing`).
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

/**
 * What waits for "Discard changes": opening another rule (or none), or
 * closing the window (`close` closes it for good).
 */
export type Pending =
  { kind: "edit"; editing: Editing | null } | { kind: "close"; close: () => void };

export interface RulesUiState {
  editing: Editing | null;
  /** Edits a saved rule (`null`: none); asks first while there are unsaved changes. */
  select: (id: RuleId | null) => void;
  /** The rule being edited was saved as `id` (a new rule becomes a saved one). */
  saved: (id: RuleId) => void;
  /** Starts a new rule from `rule` (a blank rule or one from a message); asks first like `select`. */
  startNew: (rule: Rule) => void;
  /** Stops editing at once, without asking (the rule was deleted). */
  forget: () => void;
  /** The message the window was opened from, `null` when opened otherwise. */
  origin: RulesOrigin | null;
  setOrigin: (origin: RulesOrigin | null) => void;
  /** The rule being edited has changes that are not saved. */
  dirty: boolean;
  setDirty: (dirty: boolean) => void;
  /** What waits for the user's answer to "Discard changes?", else `null`. */
  pending: Pending | null;
  /** Asks before `pending` happens while there are unsaved changes; else does it now. */
  guard: (pending: Pending) => void;
  /** Drops the unsaved changes and does what was waiting. */
  discard: () => void;
  /** Keeps the unsaved changes; what was waiting does not happen. */
  keepEditing: () => void;
  /** The editor asks a question (the rule list is `inert` meanwhile). */
  asking: boolean;
  setAsking: (asking: boolean) => void;
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
    /** Does what waited (or was asked for while nothing is unsaved). */
    const apply = (pending: Pending) => {
      if (pending.kind === "close") {
        store?.setState({ pending: null, dirty: false });
        pending.close();
      } else {
        store?.setState({ pending: null, dirty: false, editing: pending.editing });
      }
    };
    store = createStore<RulesUiState>((set, get) => ({
      editing: null,
      select: (id) => {
        const editing = get().editing;
        // The rule open already stays as it is (with its changes).
        if (id !== null && editing?.kind === "saved" && editing.id === id) return;
        get().guard({
          kind: "edit",
          editing: id === null ? null : { kind: "saved", id, key: `saved-${id}` },
        });
      },
      saved: (id) => {
        set((s) => ({ editing: { kind: "saved", id, key: s.editing?.key ?? `saved-${id}` } }));
      },
      startNew: (rule) => {
        get().guard({
          kind: "edit",
          editing: { kind: "new", rule, key: `new-${String(++seq)}` },
        });
      },
      forget: () => {
        set({ editing: null, dirty: false, pending: null });
      },
      origin: null,
      setOrigin: (origin) => {
        set({ origin });
      },
      dirty: false,
      setDirty: (dirty) => {
        if (get().dirty !== dirty) set({ dirty });
      },
      pending: null,
      guard: (pending) => {
        if (get().dirty) set({ pending });
        else apply(pending);
      },
      discard: () => {
        const pending = get().pending;
        if (pending) apply(pending);
      },
      keepEditing: () => {
        set({ pending: null });
      },
      asking: false,
      setAsking: (asking) => {
        if (get().asking !== asking) set({ asking });
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
