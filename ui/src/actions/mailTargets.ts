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
import type { NailClient } from "../api/client";
import type {
  FolderId,
  MailList,
  MessageId,
  MessageSummary,
  MessageTarget,
  ThreadId,
} from "../api/generated";
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

/**
 * The folder a whole list is in (`MailList::folder_id`, pinned by the
 * `mail-actions` ts-vectors); `null` for a search without a folder.
 */
export function listFolderOf(list: MailList): FolderId | null {
  return list.source === "search" ? list.query.folder_id : list.folder_id;
}

/** The target that names one message. */
export const messageTarget = (id: MessageId): MessageTarget => ({
  kind: "message",
  message_id: id,
});

/** The target a row stands for. */
export function rowTarget(
  row: { messageId: MessageId; threadId: ThreadId | null; child: boolean },
  scope: ListScope,
): MessageTarget {
  if (!row.child && row.threadId !== null && scope.conversations) {
    return { kind: "conversation", thread_id: row.threadId, folder_id: scope.folderId };
  }
  return messageTarget(row.messageId);
}

/** Where the targets of an action come from. */
export type TargetSource = "list" | "card" | "none";

export interface ActionTargets {
  targets: MessageTarget[];
  /** `list` = the selected / picked rows (the selection moves on after a removal). */
  source: TargetSource;
}

/** Targets of a context menu's target, or `null` to use the selection. */
function menuTargets(target: ContextTarget, state: ListSelectionState): ActionTargets | null {
  if (target.card && target.messageId !== null) {
    return { targets: [messageTarget(target.messageId)], source: "card" };
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

/** One message behind a target; `summary` only when it came with the conversation. */
export interface TargetMessage {
  id: MessageId;
  summary: MessageSummary | null;
}

/**
 * The messages behind targets, each once, in order: a message target is
 * its message, a conversation every message of it in the listed folder
 * (`GetConversation`, which also brings their summaries).
 */
export async function expandTargets(
  client: NailClient,
  targets: readonly MessageTarget[],
): Promise<TargetMessage[]> {
  const out = new Map<MessageId, TargetMessage>();
  for (const target of targets) {
    // A whole list is resolved by the core only (ADR 0235).
    if (target.kind === "list") continue;
    if (target.kind === "message") {
      if (!out.has(target.message_id))
        out.set(target.message_id, { id: target.message_id, summary: null });
      continue;
    }
    const answer = await client.request(
      { type: "GetConversation", thread_id: target.thread_id, folder_id: target.folder_id },
      "Conversation",
    );
    for (const m of answer.messages) out.set(m.id, { id: m.id, summary: m });
  }
  return [...out.values()];
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

/** What a list query says about its data (TanStack Query). */
export interface ListFetchState {
  /** When the shown data arrived (`Date.now()`), 0 = never. */
  dataUpdatedAt: number;
  isFetching: boolean;
}

/**
 * Whether the list's data was fetched again after the core said events were
 * lost (`Resync` at `resyncAt`, ADR 0230) and nothing is still loading: the
 * selection can then be checked against the rows.
 */
export function refetchedSince(resyncAt: number | null, list: ListFetchState): boolean {
  return resyncAt !== null && !list.isFetching && list.dataUpdatedAt >= resyncAt;
}
