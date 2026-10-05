/**
 * What a mail action acts on (P3, ADR 0164): Gmail-like conversation
 * targets.
 *
 * - The list row, the reader's toolbar and the list / reader keys act on the
 *   **selected conversation**: `{kind: "conversation", thread_id,
 *   folder_id}` with the listed folder (no folder while searching).
 * - A card's own actions (its header buttons, its "More" menu, the
 *   `context:thread-view.message` menu) act on **that message**, and so do a
 *   Design B child row and a list without conversations (`threads="off"`).
 * - Several rows picked with Ctrl / Shift + click act together.
 *
 * The message list publishes how its rows map to targets (`ListScope`) and
 * the rows picked besides the selection (`picked`) in a small store per UI
 * store; the action handlers read it here.
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { FolderId, MessageId, MessageTarget, ThreadId } from "../api/generated";
import { contextTarget, type ContextTarget } from "../features/menus";
import { useUiStoreApi, type UiStore } from "../state/uiStore";

/** How the list's rows map to targets. */
export interface ListScope {
  /** The rows are conversations (`threads` is not `off`). */
  conversations: boolean;
  /** The folder the rows are listed in; `null` while searching. */
  folderId: FolderId | null;
}

/** A row picked for a multi-row action. */
export interface PickedRow {
  /** The row's key in the list. */
  key: string;
  messageId: MessageId;
  /** The conversation the row stands for (`null` = a message alone). */
  threadId: ThreadId | null;
  /** A Design B child row: always its message. */
  child: boolean;
}

export interface ListSelectionState {
  scope: ListScope;
  /** Rows picked with Ctrl / Shift + click; empty or 2+ rows (the selection included). */
  picked: readonly PickedRow[];
  setScope: (scope: ListScope) => void;
  setPicked: (rows: readonly PickedRow[]) => void;
}

/** Without a list (a pop-out, tests): every action acts on its message. */
export const NO_LIST_SCOPE: ListScope = { conversations: false, folderId: null };

const stores = new WeakMap<UiStore, StoreApi<ListSelectionState>>();

/** The list-selection store of a UI store (created on first use). */
export function listSelectionFor(ui: UiStore): StoreApi<ListSelectionState> {
  const existing = stores.get(ui);
  if (existing) return existing;
  const store = createStore<ListSelectionState>()((set, get) => ({
    scope: NO_LIST_SCOPE,
    picked: [],
    setScope: (scope) => {
      const old = get().scope;
      if (old.conversations !== scope.conversations || old.folderId !== scope.folderId) {
        // Another folder or mode: picked rows do not carry over.
        set({ scope, picked: [] });
      }
    },
    setPicked: (rows) => {
      set({ picked: rows.length > 1 ? [...rows] : [] });
    },
  }));
  stores.set(ui, store);
  return store;
}

export function useListSelection<T>(selector: (state: ListSelectionState) => T): T {
  return useStore(listSelectionFor(useUiStoreApi()), selector);
}

/** The target a row stands for. */
export function rowTarget(
  row: { messageId: MessageId; threadId: ThreadId | null; child: boolean },
  scope: ListScope,
): MessageTarget {
  if (!row.child && row.threadId !== null && scope.conversations) {
    return { kind: "conversation", thread_id: row.threadId, folder_id: scope.folderId };
  }
  return { kind: "message", message_id: row.messageId };
}

/** Where the targets of an action come from. */
export type TargetSource = "list" | "card" | "none";

export interface ActionTargets {
  targets: MessageTarget[];
  /** `list` = the selected / picked rows (the selection moves on after a removal). */
  source: TargetSource;
}

const message = (id: MessageId): MessageTarget => ({ kind: "message", message_id: id });

/** Targets of a context menu's target, or `null` to use the selection. */
function menuTargets(target: ContextTarget, state: ListSelectionState): ActionTargets | null {
  if (target.card && target.messageId !== null) {
    return { targets: [message(target.messageId)], source: "card" };
  }
  if (target.part !== "message-list.row" || target.messageId === null) return null;
  const { picked, scope } = state;
  const inPicked = picked.some((p) => p.messageId === target.messageId && p.child === target.child);
  if (inPicked) return { targets: picked.map((p) => rowTarget(p, scope)), source: "list" };
  return {
    targets: [
      rowTarget(
        { messageId: target.messageId, threadId: target.threadId, child: target.child },
        scope,
      ),
    ],
    source: "list",
  };
}

/**
 * The targets of a mail action now: the open context menu's (a row, a
 * card), else the picked rows, else the selection — the selected
 * conversation when the list shows conversations, else the message.
 */
export function currentTargets(ui: UiStore): ActionTargets {
  const state = listSelectionFor(ui).getState();
  const target = contextTarget(ui);
  if (target) {
    const fromMenu = menuTargets(target, state);
    if (fromMenu) return fromMenu;
  }
  if (state.picked.length > 1) {
    return { targets: state.picked.map((p) => rowTarget(p, state.scope)), source: "list" };
  }
  const { selectedMessageId, selectedThreadId } = ui.getState();
  if (selectedMessageId === null) return { targets: [], source: "none" };
  return {
    targets: [
      rowTarget(
        { messageId: selectedMessageId, threadId: selectedThreadId, child: false },
        state.scope,
      ),
    ],
    source: "list",
  };
}

/** Whether an action has anything to act on. */
export function hasTargets(ui: UiStore): boolean {
  return currentTargets(ui).targets.length > 0;
}

/** The message ids of the message targets (for actions on single messages). */
export function messageIdsOf(targets: readonly MessageTarget[]): MessageId[] {
  return targets.flatMap((t) => (t.kind === "message" ? [t.message_id] : []));
}

/** The thread ids of the conversation targets. */
export function threadIdsOf(targets: readonly MessageTarget[]): ThreadId[] {
  return targets.flatMap((t) => (t.kind === "conversation" ? [t.thread_id] : []));
}

/** A list row as far as picking goes. */
export interface PickableRow {
  key: string;
  kind: "message" | "conversation" | "child";
  messageId: MessageId;
  threadId: ThreadId | null;
}

/** The picked form of a row. */
export function pickedOf(row: PickableRow): PickedRow {
  return {
    key: row.key,
    messageId: row.messageId,
    threadId: row.kind === "child" ? null : row.threadId,
    child: row.kind === "child",
  };
}

/**
 * Ctrl + click: adds the row to the picked rows or takes it out. Without
 * picked rows the selected row (`current`) is picked first, so the click
 * makes two.
 */
export function togglePick(
  picked: readonly PickedRow[],
  current: PickableRow | undefined,
  row: PickableRow,
): PickedRow[] {
  const base = picked.length > 0 ? [...picked] : current !== undefined ? [pickedOf(current)] : [];
  return base.some((p) => p.key === row.key)
    ? base.filter((p) => p.key !== row.key)
    : [...base, pickedOf(row)];
}

/** Shift + click (or Shift + arrow): every row from `from` to `to`, both included. */
export function rangePick(rows: readonly PickableRow[], from: number, to: number): PickedRow[] {
  if (from < 0) return to >= 0 && rows[to] ? [pickedOf(rows[to])] : [];
  const [start, end] = from <= to ? [from, to] : [to, from];
  return rows.slice(start, end + 1).map(pickedOf);
}

/** The picked rows still listed (a refetch may drop some). */
export function prunePicked(
  picked: readonly PickedRow[],
  rows: readonly { key: string }[],
): PickedRow[] {
  if (picked.length === 0) return [];
  const keys = new Set(rows.map((r) => r.key));
  return picked.filter((p) => keys.has(p.key));
}
