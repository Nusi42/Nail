/**
 * The reader's card feed (P2 Design A, WP203): which cards start open,
 * which collapsed ones fold into "N older messages", where the keyboard
 * goes next, and the per-conversation state the keys and the widget share.
 *
 * The rules (ADR 0148, plan "Wave P2"):
 *
 * - Cards run oldest → newest. Unread messages, the newest message and the
 *   message chosen in the list start open; read older ones start collapsed
 *   to one line. What starts open is decided once per message (when the
 *   feed first sees it), so reading a card never collapses it again.
 * - More than four collapsed cards in a row in the middle (never the first
 *   or the newest card) fold into one "N older messages" button; the
 *   collapsed card right before the next open one stays as context.
 * - j / k step through the visible stops (cards and folds), n / p through
 *   the messages, opening a fold on the way.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

/** The reader's per-message widgets an open card shows, in this order by default (`card=`). */
export const CARD_WIDGETS = [
  "security-message",
  "security-warnings",
  "remote-content",
  "junk-verdict",
  "sync-message-body",
  "message-view",
] as const;

/** Splits text into runs of quoted (`>`) and own lines. */
export function splitQuotes(text: string): { quoted: boolean; text: string }[] {
  const blocks: { quoted: boolean; text: string }[] = [];
  for (const line of text.split("\n")) {
    const quoted = line.startsWith(">");
    const last = blocks[blocks.length - 1];
    if (last?.quoted === quoted) last.text += `\n${line}`;
    else blocks.push({ quoted, text: line });
  }
  return blocks;
}

/** The fields of a conversation message the feed rules read. */
export interface FeedMessage {
  id: string;
  flags: { seen: boolean };
}

/** Collapsed cards in a row before they fold: more than this many fold. */
export const FOLD_AFTER = 4;

/**
 * The messages that start open: the unread ones, the newest one and the
 * selected one (the list's choice).
 */
export function initiallyOpen(
  messages: readonly FeedMessage[],
  selectedId: string | null,
): Set<string> {
  const open = new Set<string>();
  for (const m of messages) if (!m.flags.seen) open.add(m.id);
  const newest = messages[messages.length - 1];
  if (newest) open.add(newest.id);
  if (selectedId !== null && messages.some((m) => m.id === selectedId)) open.add(selectedId);
  return open;
}

/**
 * The card the feed scrolls to when it opens: the selected message when the
 * list chose an older one, else the newest unread, else the newest.
 */
export function primaryMessage(
  messages: readonly FeedMessage[],
  selectedId: string | null,
): string | null {
  const newest = messages[messages.length - 1];
  if (!newest) return null;
  if (selectedId !== null && selectedId !== newest.id && messages.some((m) => m.id === selectedId))
    return selectedId;
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i];
    if (m && !m.flags.seen) return m.id;
  }
  return newest.id;
}

/** One entry of the feed: a card or a fold standing for collapsed cards. */
export type FeedItem<T extends FeedMessage> =
  { kind: "card"; message: T; index: number } | { kind: "fold"; messages: T[]; index: number };

/**
 * The feed's entries. `isOpen` says whether a card is open; `keep` is never
 * folded away (the selected card); `unfolded` shows every card.
 */
export function feedItems<T extends FeedMessage>(
  messages: readonly T[],
  isOpen: (m: T) => boolean,
  keep: string | null,
  unfolded: boolean,
): FeedItem<T>[] {
  const cards = messages.map((message, index) => ({ kind: "card" as const, message, index }));
  if (unfolded || messages.length < FOLD_AFTER + 3) return cards;
  const out: FeedItem<T>[] = [];
  let run: { message: T; index: number }[] = [];
  const flush = () => {
    const last = run.length - 1;
    // The kept card is never folded away: a run that would hide it shows.
    const hidesKept = run.slice(0, last).some((r) => r.message.id === keep);
    if (run.length > FOLD_AFTER && !hidesKept) {
      out.push({
        kind: "fold",
        messages: run.slice(0, last).map((r) => r.message),
        index: run[0]?.index ?? 0,
      });
      const context = run[last];
      if (context) out.push({ kind: "card", ...context });
    } else {
      for (const r of run) out.push({ kind: "card", ...r });
    }
    run = [];
  };
  messages.forEach((message, index) => {
    const middle = index > 0 && index < messages.length - 1;
    if (middle && !isOpen(message)) {
      run.push({ message, index });
      return;
    }
    flush();
    out.push({ kind: "card", message, index });
  });
  flush();
  return out;
}

/** The ids of the cards a fold stands for, by fold. */
export function foldedIds<T extends FeedMessage>(items: readonly FeedItem<T>[]): Set<string> {
  const ids = new Set<string>();
  for (const item of items) if (item.kind === "fold") for (const m of item.messages) ids.add(m.id);
  return ids;
}

/**
 * The next stop of j / k (`delta` 1 / -1) from `current` among `stops` (in
 * feed order), or `null` at either end. Without a current stop, j starts at
 * the first and k at the last.
 */
export function stepStop<T>(stops: readonly T[], current: T | null, delta: 1 | -1): T | null {
  if (stops.length === 0) return null;
  const index = current === null ? -1 : stops.indexOf(current);
  if (index < 0) return (delta > 0 ? stops[0] : stops[stops.length - 1]) ?? null;
  return stops[index + delta] ?? null;
}

// ---------------------------------------------------------------------------
// Per-conversation state, shared by the widget and the reader's actions.

interface FeedStoreState {
  /** Per conversation: message id → open. */
  open: Record<string, Record<string, boolean>>;
  /** Per conversation: its message ids, oldest first (n / p). */
  order: Record<string, readonly string[]>;
  /** Conversations whose folds are opened. */
  unfolded: Record<string, boolean>;
  /** A card the widget should focus after the next render (n / p across a fold). */
  focus: { threadId: string; messageId: string; seq: number } | null;
  /**
   * Remembers the conversation's messages (`order`) and what starts open
   * for those the feed has not seen yet.
   */
  seed: (
    threadId: string,
    order: readonly string[],
    open: Readonly<Record<string, boolean>>,
  ) => void;
  setOpen: (threadId: string, ids: readonly string[], open: boolean) => void;
  unfold: (threadId: string) => void;
  requestFocus: (threadId: string, messageId: string) => void;
  reset: () => void;
}

const EMPTY = { open: {}, order: {}, unfolded: {}, focus: null };

export const feedStore = createStore<FeedStoreState>()((set) => ({
  ...EMPTY,
  seed: (threadId, order, open) => {
    set((s) => {
      const known = s.open[threadId] ?? {};
      const missing = order.filter((id) => !Object.hasOwn(known, id));
      const before = s.order[threadId] ?? [];
      const sameOrder = before.length === order.length && before.every((id, i) => id === order[i]);
      if (missing.length === 0 && sameOrder) return s;
      const next = { ...known };
      for (const id of missing) next[id] = open[id] ?? false;
      return {
        open: { ...s.open, [threadId]: next },
        order: sameOrder ? s.order : { ...s.order, [threadId]: [...order] },
      };
    });
  },
  setOpen: (threadId, ids, value) => {
    set((s) => {
      const next = { ...(s.open[threadId] ?? {}) };
      for (const id of ids) next[id] = value;
      return { open: { ...s.open, [threadId]: next } };
    });
  },
  unfold: (threadId) => {
    set((s) => (s.unfolded[threadId] ? s : { unfolded: { ...s.unfolded, [threadId]: true } }));
  },
  requestFocus: (threadId, messageId) => {
    set((s) => ({ focus: { threadId, messageId, seq: (s.focus?.seq ?? 0) + 1 } }));
  },
  reset: () => {
    set(EMPTY);
  },
}));

/** Subscribes to a slice of the feed state. */
export function useFeed<T>(selector: (state: FeedStoreState) => T): T {
  return useStore(feedStore, selector);
}
