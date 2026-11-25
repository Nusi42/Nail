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
 * - Ctrl+A (R5, ADR 0235) selects the whole list, rows not loaded yet
 *   included: the selection is then "all except these" (`all`), and an
 *   action sends one list target (`MessageTarget::List`) instead of ids.
 *   Ctrl / Shift + click then leaves rows out or takes them back.
 *
 * The message list publishes how its rows map to targets (`ListScope`) and
 * the rows picked besides the selection (`picked`) in a small store per UI
 * store; the action handlers read it here. Which target a row stands for
 * is `./rowTargets.ts` (shared with the mock core).
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
import { messageTarget, rowTarget, type ListScope } from "./rowTargets";
import { searchQueryOf, type SearchScope } from "../state/queries";
import { useUiStoreApi, type UiStore, UiStoreMap } from "../state/uiStore";
import { sameJson } from "../text/json";

export {
  listRowTarget,
  messageTarget,
  rowFolderOf,
  rowTarget,
  type ListScope,
  type RowTarget,
} from "./rowTargets";

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

/** "Select all" (R5): the whole list except some rows. */
export interface AllSelection {
  /** The list as the message list pages it. */
  list: MailList;
  /** The rows left out (Ctrl / Shift + click after "select all"). */
  except: readonly PickedRow[];
}

export interface ListSelectionState {
  scope: ListScope;
  /** The list the message list shows (`null` = none, e.g. no folder). */
  list: MailList | null;
  /** Rows picked with Ctrl / Shift + click; empty or 2+ rows (the selection included). */
  picked: readonly PickedRow[];
  /** The whole list is selected (`null` = the explicit selection applies). */
  all: AllSelection | null;
  setScope: (scope: ListScope) => void;
  /** The list shown now; another list ends "select all". */
  setList: (list: MailList | null) => void;
  setPicked: (rows: readonly PickedRow[]) => void;
  /** Selects the whole list shown (nothing without a list). */
  selectAll: () => void;
  /** The rows "select all" leaves out. */
  setExcept: (rows: readonly PickedRow[]) => void;
  /** Ends "select all" and drops the picked rows. */
  clear: () => void;
}

/** Without a list (a pop-out, tests): every action acts on its message. */
export const NO_LIST_SCOPE: ListScope = { conversations: false, folderId: null };

const stores = new UiStoreMap<StoreApi<ListSelectionState>>();

/** The list-selection store of a UI store (created on first use). */
export function listSelectionFor(ui: UiStore): StoreApi<ListSelectionState> {
  return stores.obtain(ui, () => {
    const store = createStore<ListSelectionState>()((set, get) => ({
      scope: NO_LIST_SCOPE,
      list: null,
      picked: [],
      all: null,
      setScope: (scope) => {
        const old = get().scope;
        if (old.conversations !== scope.conversations || old.folderId !== scope.folderId) {
          // Another folder or mode: picked rows do not carry over.
          set({ scope, picked: [], all: null });
        }
      },
      setList: (list) => {
        if (sameJson(get().list, list)) return;
        // Another folder, search or filter: "select all" meant the old list.
        set({ list, all: null });
      },
      setPicked: (rows) => {
        set({ picked: rows.length > 1 ? [...rows] : [], all: null });
      },
      selectAll: () => {
        const { list } = get();
        if (list !== null) set({ all: { list, except: [] }, picked: [] });
      },
      setExcept: (rows) => {
        const { all } = get();
        if (all !== null) set({ all: { list: all.list, except: [...rows] } });
      },
      clear: () => {
        set({ all: null, picked: [] });
      },
    }));
    return store;
  });
}

export function useListSelection<T>(selector: (state: ListSelectionState) => T): T {
  return useStore(listSelectionFor(useUiStoreApi()), selector);
}

/** What the message list shows, as far as its list query goes. */
export interface ListView {
  /** The search the list runs (the typed one with the filter chips' terms). */
  search: SearchScope | null;
  /**
   * The user typed a search (else `search` is a filter chip's term within
   * the folder): its conversation rows have no folder.
   */
  typedSearch: boolean;
  folderId: FolderId | null;
  /** The rows are conversations (`ListThreads`), else messages. */
  threadRows: boolean;
  /** Search hits stand for their conversations (a list with conversations). */
  conversations: boolean;
  pageSize: number;
}

/**
 * The list query of what the message list shows (`MailList`, ADR 0235):
 * the very query it pages with; `null` without a folder or search.
 */
export function mailListOf(view: ListView): MailList | null {
  if (view.search !== null) {
    return {
      source: "search",
      query: searchQueryOf(view.search, 0, view.pageSize),
      conversations: view.conversations,
      // A filter chip lists the folder's rows (filtered); a typed search's
      // rows have no folder.
      listed_folder_id: view.typedSearch ? null : view.folderId,
    };
  }
  if (view.folderId === null) return null;
  return view.threadRows
    ? { source: "conversations", folder_id: view.folderId }
    : { source: "messages", folder_id: view.folderId };
}

/**
 * The folder a whole list is in (`MailList::folder_id`, pinned by the
 * `mail-actions` ts-vectors); `null` for a search without a folder.
 */
export function listFolderOf(list: MailList): FolderId | null {
  return list.source === "search" ? list.query.folder_id : list.folder_id;
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
  const { picked, scope, all } = state;
  if (all !== null) {
    // A row of "select all" stands for the whole selection; a row left
    // out for itself.
    const left = all.except.some(
      (p) => p.messageId === target.messageId && p.child === target.child,
    );
    if (!left) return { targets: [listTargetOf(all, scope)], source: "list" };
  }
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
  if (state.all !== null) {
    return { targets: [listTargetOf(state.all, state.scope)], source: "list" };
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

// ---- "Select all" (R5, ADR 0235) ----

/** The list target of "select all": the list minus the rows left out. */
export function listTargetOf(all: AllSelection, scope: ListScope): MessageTarget {
  return {
    kind: "list",
    list: all.list,
    except: all.except.map((p) => rowTarget(p, scope)),
    as_of: null,
  };
}

/** Whether a target names a whole list. */
export function hasListTarget(targets: readonly MessageTarget[]): boolean {
  return targets.some((t) => t.kind === "list");
}

/**
 * The targets with every list target's `as_of` set to a confirmation's
 * mark: the action then reaches only the messages the core counted.
 */
export function withAsOf(targets: readonly MessageTarget[], asOf: string | null): MessageTarget[] {
  return targets.map((t) => (t.kind === "list" ? { ...t, as_of: asOf } : t));
}

/** Whether a row is selected under "select all" (not left out). */
export function inAll(all: AllSelection, key: string): boolean {
  return !all.except.some((p) => p.key === key);
}

/** Ctrl + click under "select all": leaves the row out, or takes it back. */
export function toggleExcept(except: readonly PickedRow[], row: PickableRow): PickedRow[] {
  return except.some((p) => p.key === row.key)
    ? except.filter((p) => p.key !== row.key)
    : [...except, pickedOf(row)];
}

/**
 * Shift + click under "select all": the rows from `from` to `to` are left
 * out when the clicked row was selected, else taken back.
 */
export function rangeExcept(
  except: readonly PickedRow[],
  rows: readonly PickableRow[],
  from: number,
  to: number,
): PickedRow[] {
  const range = rangePick(rows, from, to);
  const target = rows[to];
  const keys = new Set(range.map((p) => p.key));
  const rest = except.filter((p) => !keys.has(p.key));
  const selected = target !== undefined && !except.some((p) => p.key === target.key);
  return selected ? [...rest, ...range] : rest;
}

/**
 * The loaded rows a list target stands for, as row targets: what the list
 * and the caches update at once (the core acts on the whole list).
 * `order` / `threads` are the list's rows (`UiState.messageOrder` /
 * `messageThreads`); a row named in the target's exceptions is left out.
 */
export function loadedRowTargets(
  target: MessageTarget,
  order: readonly MessageId[],
  threads: Readonly<Record<MessageId, ThreadId>>,
  scope: ListScope,
): MessageTarget[] {
  if (target.kind !== "list") return [target];
  const leftMessages = new Set(messageIdsOf(target.except));
  const leftThreads = new Set(threadIdsOf(target.except));
  return order.flatMap((id) => {
    const thread = threads[id] ?? null;
    if (leftMessages.has(id) || (thread !== null && leftThreads.has(thread))) return [];
    return [rowTarget({ messageId: id, threadId: thread, child: false }, scope)];
  });
}
