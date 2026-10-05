/**
 * The card feed's keys (P2 WP203; `keys context="reader"` in
 * `defaults/layout.kdl`): j / k step through the feed's stops (card
 * headers and folds), n / p through the conversation's messages (opening a
 * fold on the way), o opens or closes the focused card (Enter does the same
 * on a card's header button), ; / : open or close every card. Moving to a
 * card selects its message, so r / a / f (the global reply keys) act on it.
 *
 * The actions work on the feed in the document: the one around the focus,
 * else the first one shown. Without a feed (a layout of the strip, a lone
 * message) j / k step through the list like `mail.next` / `mail.previous`.
 */
import type { ActionInfo } from "../../actions/palette";
import { builtinActions, type ActionContext, type ActionHandler } from "../../actions/registry";
import { feedStore, stepStop } from "../../widgets/ThreadViewModel";

export const CONVERSATION_ACTIONS = {
  nextCard: "conversation.next-card",
  previousCard: "conversation.previous-card",
  nextMessage: "conversation.next-message",
  previousMessage: "conversation.previous-message",
  toggleCard: "conversation.toggle-card",
  expandAll: "conversation.expand-all",
  collapseAll: "conversation.collapse-all",
} as const;

const FEED = '[data-part="thread-view"][data-mode="cards"]';
const CARD = '[data-part="thread-view.card"]';
const TOGGLE = '[data-part="thread-view.card.toggle"]';
const STOP = "[data-feed-stop]";

/** The feed the keys act on, or `null` when none is shown. */
export function activeFeed(doc: Document = document): HTMLElement | null {
  const active = doc.activeElement;
  return active?.closest<HTMLElement>(FEED) ?? doc.querySelector<HTMLElement>(FEED);
}

/** The card the keys act on: the focused one, else the selected one. */
export function currentCard(feed: HTMLElement): HTMLElement | null {
  const active = feed.ownerDocument.activeElement;
  const focused = active && feed.contains(active) ? active.closest<HTMLElement>(CARD) : null;
  return focused ?? feed.querySelector<HTMLElement>(`${CARD}[aria-current="true"]`);
}

function focusCard(feed: HTMLElement, messageId: string): boolean {
  const toggle = feed.querySelector<HTMLElement>(
    `${CARD}[data-message="${CSS.escape(messageId)}"] ${TOGGLE}`,
  );
  toggle?.focus();
  return toggle !== null;
}

function stepStops(ctx: ActionContext, delta: 1 | -1): void {
  const feed = activeFeed();
  if (feed === null) {
    void builtinActions[delta > 0 ? "mail.next" : "mail.previous"]?.run(ctx, []);
    return;
  }
  const stops = [...feed.querySelectorAll<HTMLElement>(STOP)];
  const active = feed.ownerDocument.activeElement;
  let current: HTMLElement | null =
    active instanceof HTMLElement && stops.includes(active) ? active : null;
  current ??= currentCard(feed)?.querySelector<HTMLElement>(TOGGLE) ?? null;
  stepStop(stops, current, delta)?.focus();
}

function stepMessages(delta: 1 | -1): void {
  const feed = activeFeed();
  const threadId = feed?.dataset.thread;
  if (!feed || threadId === undefined) return;
  const order = feedStore.getState().order[threadId] ?? [];
  const current = currentCard(feed)?.dataset.message ?? null;
  const next = stepStop(order, current, delta);
  if (next === null) return;
  if (!focusCard(feed, next)) {
    // Behind a fold: open it, the widget focuses the card once it shows.
    feedStore.getState().unfold(threadId);
    feedStore.getState().requestFocus(threadId, next);
  }
}

function setAll(open: boolean): void {
  const feed = activeFeed();
  const threadId = feed?.dataset.thread;
  if (threadId === undefined) return;
  const state = feedStore.getState();
  state.setOpen(threadId, state.order[threadId] ?? [], open);
  if (open) state.unfold(threadId);
}

/** Action ids → handlers. */
export const conversationActions: Record<string, ActionHandler> = {
  [CONVERSATION_ACTIONS.nextCard]: {
    run: (ctx) => {
      stepStops(ctx, 1);
    },
  },
  [CONVERSATION_ACTIONS.previousCard]: {
    run: (ctx) => {
      stepStops(ctx, -1);
    },
  },
  [CONVERSATION_ACTIONS.nextMessage]: {
    run: () => {
      stepMessages(1);
    },
  },
  [CONVERSATION_ACTIONS.previousMessage]: {
    run: () => {
      stepMessages(-1);
    },
  },
  [CONVERSATION_ACTIONS.toggleCard]: {
    run: () => {
      const feed = activeFeed();
      const card = feed ? currentCard(feed) : null;
      card?.querySelector<HTMLElement>(TOGGLE)?.click();
    },
  },
  [CONVERSATION_ACTIONS.expandAll]: {
    run: () => {
      setAll(true);
    },
  },
  [CONVERSATION_ACTIONS.collapseAll]: {
    run: () => {
      setAll(false);
    },
  },
};

const reader = ["reader"] as const;

/** Command-palette descriptions. */
export const conversationActionInfo: Record<string, ActionInfo> = {
  [CONVERSATION_ACTIONS.nextCard]: {
    title: "thread-view-action-next-card",
    availability: reader,
    icon: "chevron-down",
  },
  [CONVERSATION_ACTIONS.previousCard]: {
    title: "thread-view-action-previous-card",
    availability: reader,
    icon: "chevron-up",
  },
  [CONVERSATION_ACTIONS.nextMessage]: {
    title: "thread-view-action-next-message",
    availability: reader,
    icon: "chevron-right",
  },
  [CONVERSATION_ACTIONS.previousMessage]: {
    title: "thread-view-action-previous-message",
    availability: reader,
    icon: "chevron-left",
  },
  [CONVERSATION_ACTIONS.toggleCard]: {
    title: "thread-view-action-toggle-card",
    availability: reader,
    icon: "rows-2",
  },
  [CONVERSATION_ACTIONS.expandAll]: {
    title: "thread-view-action-expand-all",
    availability: reader,
    icon: "maximize-2",
  },
  [CONVERSATION_ACTIONS.collapseAll]: {
    title: "thread-view-action-collapse-all",
    availability: reader,
    icon: "minimize-2",
  },
};
