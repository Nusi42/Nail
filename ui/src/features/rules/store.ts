/**
 * UI state of the rules feature: which rule is being edited, a rule
 * pre-filled by "create rule from message" waiting for the editor, the
 * message the window was opened from, and the outcome of a run from a menu
 * (R1). One store per client (so every app instance, and every test, has
 * its own).
 *
 * Unsaved changes (R2 design review, item 4; R6, ADR 0239): the editor
 * reports whether its rule differs from the saved one (`setDirty`) and how
 * to ask before it is left (`setLeave`: the shared unsaved-changes prompt,
 * `./RulesGuard.tsx`). While it is dirty, opening another rule (`select`,
 * `startNew`) waits for the prompt's Save or Don't save; Cancel keeps the
 * rule. Closing the window asks through the window layer's close guard.
 *
 * The editor's questions (delete a rule, run one that moves or deletes
 * mail) open as modal confirmation windows (`question`, `./Questions.tsx`).
 *
 * Multi-select (R3, ADR 0227): the list's selected rules (`selected`, with
 * the `anchor` Shift extends from); the editor shows the focused one
 * (`editing`). A pick that shows another rule waits for the same question.
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { NailClient } from "../../api/client";
import type { FolderId, MessageId, Rule, RuleId, RuleRun } from "../../api/generated";
import { useClient } from "../../state/queries";
import type { RunTargetKind } from "./runTargets";
import type { ListSelection } from "../../widgets/listSelection";

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

/** The list's selected rules and the anchor of a range (the focus is `editing`). */
export type RuleSelection = Pick<ListSelection, "selected" | "anchor">;

/** Nothing selected. */
const NO_SELECTION: RuleSelection = { selected: [], anchor: null };

/** Only `id` selected (nothing for `null`). */
const only = (id: RuleId | null): RuleSelection =>
  id === null ? NO_SELECTION : { selected: [id], anchor: id };

/** What waits for the unsaved-changes prompt: opening another rule (or none) with the list's selection. */
interface Pending {
  editing: Editing | null;
  selection: RuleSelection;
}

/**
 * A question of the editor, asked in a modal confirmation window
 * (`./Questions.tsx`): deleting the shown rule, or a run that moves or
 * deletes mail (the rules that run, their total preview count and the
 * target's name). `go` does it.
 */
export type RuleQuestion =
  | { kind: "delete"; name: string; go: () => void }
  | {
      kind: "run";
      names: readonly string[];
      count: number;
      target: string;
      /** The verb ("Run", "Save and run 3 rules"), localized. */
      action: string;
      go: () => void;
    };

export interface RulesUiState {
  editing: Editing | null;
  /** The rules selected in the list (in the order picked), and the anchor of a range. */
  selection: RuleSelection;
  /** Edits a saved rule (`null`: none), selecting only it; asks first while there are unsaved changes. */
  select: (id: RuleId | null) => void;
  /**
   * Selects rules in the list and edits the focused one; asks first while
   * there are unsaved changes and the focus moves to another rule.
   */
  choose: (selection: ListSelection) => void;
  /** The rule being edited was saved as `id` (a new rule becomes a saved one). */
  saved: (id: RuleId) => void;
  /** Starts a new rule from `rule` (a blank rule or one from a message); asks first like `select`. */
  startNew: (rule: Rule) => void;
  /** Stops editing at once, without asking (the rule was deleted). */
  forget: () => void;
  /**
   * Counts the calls of `select` and `startNew` (even of a rule already
   * shown): a popped-out rules window gets the rule only when one was
   * asked for since its last handover (`./popOut.ts`).
   */
  picks: number;
  /** The message the window was opened from, `null` when opened otherwise. */
  origin: RulesOrigin | null;
  /** Sets the origin; "Run on" starts over from its default. */
  setOrigin: (origin: RulesOrigin | null) => void;
  /**
   * The user's "Run on" choice (`null`: the default), kept while other
   * rules are picked in the list (R3), so it holds for several rules.
   */
  runOn: RunTargetKind | null;
  setRunOn: (runOn: RunTargetKind | null) => void;
  /** The rule being edited has changes that are not saved. */
  dirty: boolean;
  setDirty: (dirty: boolean) => void;
  /**
   * How the shown editor asks before it is left (`proceed` runs after the
   * shared prompt's Save or Don't save); `null` without an editor.
   */
  leave: ((proceed: () => void) => void) | null;
  setLeave: (leave: ((proceed: () => void) => void) | null) => void;
  /** The editor's open question, else `null`. */
  question: RuleQuestion | null;
  setQuestion: (question: RuleQuestion | null) => void;
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
      store?.setState({ dirty: false, editing: pending.editing, selection: pending.selection });
    };
    /** Opens another rule; asks first while the shown one has unsaved changes. */
    const guard = (pending: Pending) => {
      const leave = store?.getState().leave ?? null;
      if (store?.getState().dirty === true && leave !== null) {
        leave(() => {
          apply(pending);
        });
      } else apply(pending);
    };
    store = createStore<RulesUiState>((set, get) => ({
      editing: null,
      selection: NO_SELECTION,
      select: (id) => {
        set((s) => ({ picks: s.picks + 1 }));
        const editing = get().editing;
        // The rule open already stays as it is (with its changes).
        if (id !== null && editing?.kind === "saved" && editing.id === id) {
          set({ selection: only(id) });
          return;
        }
        guard({
          editing: id === null ? null : { kind: "saved", id, key: `saved-${id}` },
          selection: only(id),
        });
      },
      choose: ({ selected, anchor, focus }) => {
        const editing = get().editing;
        const selection = { selected, anchor };
        // The focused rule is open already: only the selection changes.
        if (editing?.kind === "saved" && editing.id === focus) {
          set({ selection });
          return;
        }
        set((s) => ({ picks: s.picks + 1 }));
        guard({
          editing: { kind: "saved", id: focus, key: `saved-${focus}` },
          selection,
        });
      },
      saved: (id) => {
        set((s) => ({
          editing: { kind: "saved", id, key: s.editing?.key ?? `saved-${id}` },
          // A new rule, once saved, is the one selected.
          selection: s.editing?.kind === "new" ? only(id) : s.selection,
        }));
      },
      startNew: (rule) => {
        set((s) => ({ picks: s.picks + 1 }));
        guard({
          editing: { kind: "new", rule, key: `new-${String(++seq)}` },
          selection: NO_SELECTION,
        });
      },
      forget: () => {
        set({ editing: null, dirty: false, selection: NO_SELECTION });
      },
      picks: 0,
      origin: null,
      setOrigin: (origin) => {
        set({ origin, runOn: null });
      },
      runOn: null,
      setRunOn: (runOn) => {
        set({ runOn });
      },
      dirty: false,
      setDirty: (dirty) => {
        if (get().dirty !== dirty) set({ dirty });
      },
      leave: null,
      setLeave: (leave) => {
        set({ leave });
      },
      question: null,
      setQuestion: (question) => {
        set({ question });
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
