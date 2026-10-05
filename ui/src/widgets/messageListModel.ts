/**
 * The message list's row model (P2 WP202, ADR 0148): the rows the list
 * shows — messages, conversations (Design A) and, with
 * `threads="expandable"` (Design B), the child rows of the expanded
 * conversations, one level deep — and the selection model over them.
 *
 * A child row and its conversation's row can stand for the same message
 * (the conversation row selects its newest message), so the list keeps
 * the chosen row by key and falls back to the first row of the selected
 * message when the selection changed elsewhere (the reader's j / k, a
 * delete). Pure functions; `MessageList.tsx` renders them.
 */
import type { MessageId, ThreadId } from "../api/generated";

/** What a row stands for. */
export type RowKind = "message" | "conversation" | "child";

/** The navigation facts of one rendered row. */
export interface RowRef {
  /** Unique among the rendered rows (a child: `<thread>/<message>`). */
  key: string;
  kind: RowKind;
  /** The message selected when the row is chosen. */
  messageId: MessageId;
  /**
   * The thread the reader shows for the row: a conversation's thread, a
   * message's thread; `null` for a child row (the reader shows the one
   * message, Design B).
   */
  threadId: ThreadId | null;
  /** The conversation a child row belongs to (its row's key), else `null`. */
  parentKey: string | null;
  /** A conversation row that opens into child rows (more than one message). */
  expandable: boolean;
}

/** The key of a child row. */
export function childKey(threadId: ThreadId, messageId: MessageId): string {
  return `${threadId}/${messageId}`;
}

/**
 * The rendered rows: every base row, and below each expanded conversation
 * its children (in the conversation's order, oldest first).
 */
export function visibleRows<R extends RowRef>(
  base: readonly R[],
  expanded: ReadonlySet<string>,
  childrenOf: (row: R) => readonly R[] | undefined,
): R[] {
  const out: R[] = [];
  for (const row of base) {
    out.push(row);
    if (row.kind === "conversation" && row.expandable && expanded.has(row.key)) {
      out.push(...(childrenOf(row) ?? []));
    }
  }
  return out;
}

/**
 * The messages in row order without repeats (`UiState.messageOrder`: the
 * reader's j / k and the selection after a delete step through it).
 */
export function messageOrderOf(rows: readonly RowRef[]): MessageId[] {
  const seen = new Set<MessageId>();
  const out: MessageId[] = [];
  for (const row of rows) {
    if (seen.has(row.messageId)) continue;
    seen.add(row.messageId);
    out.push(row.messageId);
  }
  return out;
}

/**
 * The conversation of each listed message's first row (`UiState.messageThreads`;
 * child rows have none).
 */
export function messageThreadsOf(rows: readonly RowRef[]): Record<MessageId, ThreadId> {
  const out: Record<MessageId, ThreadId> = {};
  for (const row of rows) {
    if (row.threadId !== null && !(row.messageId in out)) out[row.messageId] = row.threadId;
  }
  return out;
}

/**
 * The selected row: the chosen row while it still shows the selected
 * message, else the first row of that message, else the row of the selected
 * conversation (the reader's cards select older messages, also the user's
 * own from Sent, that have no row of their own; P2); -1 when none shows it.
 */
export function selectedIndexOf(
  rows: readonly RowRef[],
  chosenKey: string | null,
  selectedId: MessageId | null,
  selectedThreadId: ThreadId | null = null,
): number {
  if (selectedId === null) return -1;
  if (chosenKey !== null) {
    const chosen = rows.findIndex((r) => r.key === chosenKey);
    if (chosen >= 0 && rows[chosen]?.messageId === selectedId) return chosen;
  }
  const own = rows.findIndex((r) => r.messageId === selectedId);
  if (own >= 0 || selectedThreadId === null) return own;
  return rows.findIndex((r) => r.kind !== "child" && r.threadId === selectedThreadId);
}

/**
 * The row j / k moves to: the next / previous one, the first (j) or the
 * last (k) without a selection; clamped at both ends.
 */
export function stepIndex(count: number, index: number, delta: number): number {
  if (count === 0) return -1;
  if (index < 0) return delta > 0 ? 0 : count - 1;
  return Math.min(count - 1, Math.max(0, index + delta));
}

/** What → / ← do on the selected row (a tree's keys, WAI-ARIA). */
export type TreeMove =
  | { kind: "expand"; key: string }
  | { kind: "collapse"; key: string }
  | { kind: "select"; index: number }
  | { kind: "none" };

/**
 * →: a closed conversation opens; an open one moves to its first child.
 * Other rows stay.
 */
export function expandMove(
  rows: readonly RowRef[],
  index: number,
  expanded: ReadonlySet<string>,
): TreeMove {
  const row = rows[index];
  if (row?.kind !== "conversation" || !row.expandable) return { kind: "none" };
  if (!expanded.has(row.key)) return { kind: "expand", key: row.key };
  const next = rows[index + 1];
  return next?.parentKey === row.key ? { kind: "select", index: index + 1 } : { kind: "none" };
}

/**
 * ←: an open conversation closes; a child row moves to its conversation.
 * Other rows stay.
 */
export function collapseMove(
  rows: readonly RowRef[],
  index: number,
  expanded: ReadonlySet<string>,
): TreeMove {
  const row = rows[index];
  if (!row) return { kind: "none" };
  if (row.kind === "child" && row.parentKey !== null) {
    const parent = rows.findIndex((r) => r.key === row.parentKey);
    return parent >= 0 ? { kind: "select", index: parent } : { kind: "none" };
  }
  if (row.kind === "conversation" && expanded.has(row.key)) {
    return { kind: "collapse", key: row.key };
  }
  return { kind: "none" };
}

/** `expanded` with `key` added or removed (a new set when it changes). */
export function toggled(
  expanded: ReadonlySet<string>,
  key: string,
  open: boolean,
): ReadonlySet<string> {
  if (expanded.has(key) === open) return expanded;
  const next = new Set(expanded);
  if (open) next.add(key);
  else next.delete(key);
  return next;
}

/** The position of a child row among its siblings (`aria-posinset` / `-setsize`). */
export function siblingPosition(
  rows: readonly RowRef[],
  index: number,
): { posinset: number; setsize: number } | null {
  const row = rows[index];
  if (row?.kind !== "child") return null;
  let first = index;
  while (first > 0 && rows[first - 1]?.parentKey === row.parentKey) first--;
  let last = index;
  while (last < rows.length - 1 && rows[last + 1]?.parentKey === row.parentKey) last++;
  return { posinset: index - first + 1, setsize: last - first + 1 };
}
