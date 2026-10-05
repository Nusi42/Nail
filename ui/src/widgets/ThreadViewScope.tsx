/**
 * One open card of the reader's feed shows the reader's per-message widgets
 * (`message-view`, security badges and warnings, the remote-content banner,
 * the junk verdict, the body state) for its own message (P2 WP203). Those
 * widgets read the selected message from the UI state; inside a card they
 * get a view of the UI state whose `selectedMessageId` is the card's
 * message (`scopedUiStore`). Everything else — and every write — is the
 * app's own store, so an action a card's widget runs still sees the app's
 * state; the feed selects a card's message as soon as the pointer or the
 * focus enters the card, so actions act on it.
 */
import { createContext, useContext, useMemo, type ReactNode } from "react";
import type { MessageId } from "../api/generated";
import { UiStoreContext, useUiStoreApi, type UiState, type UiStore } from "../state/uiStore";

/**
 * A view of `parent` in which `selectedMessageId` is `messageId`. The state
 * object stays the same between changes of the parent's (as
 * `useSyncExternalStore` requires).
 */
export function scopedUiStore(parent: UiStore, messageId: MessageId): UiStore {
  let seen: UiState | null = null;
  let scoped: UiState | null = null;
  const scope = (state: UiState): UiState =>
    state.selectedMessageId === messageId ? state : { ...state, selectedMessageId: messageId };
  const getState = (): UiState => {
    const state = parent.getState();
    if (state !== seen || scoped === null) {
      seen = state;
      scoped = scope(state);
    }
    return scoped;
  };
  return {
    getState,
    getInitialState: () => scope(parent.getInitialState()),
    setState: parent.setState,
    subscribe: (listener) =>
      parent.subscribe((_state, previous) => {
        listener(getState(), scope(previous));
      }),
  };
}

/** Whether a widget renders inside a card of the feed (and for which message). */
export interface ReaderCard {
  messageId: MessageId;
  /** The conversation's subject (the feed's heading). */
  subject: string;
}

/** A subject without its reply / forward prefixes and list tags, for comparing. */
function bareSubject(subject: string): string {
  return subject
    .replace(/^\s*((re|fwd?|aw|wg)\s*:\s*|\[[^\]]*\]\s*)+/i, "")
    .trim()
    .toLowerCase();
}

/**
 * Whether a card's message shows its own subject: when it says something
 * the feed's heading does not (a changed subject, or the real subject of an
 * encrypted message whose stored subject is a placeholder).
 */
export function cardShowsSubject(subject: string, card: ReaderCard): boolean {
  return bareSubject(subject) !== "" && bareSubject(subject) !== bareSubject(card.subject);
}

const ReaderCardContext = createContext<ReaderCard | null>(null);

/** The card around a widget, or `null` outside the feed. */
export function useReaderCard(): ReaderCard | null {
  return useContext(ReaderCardContext);
}

/** Renders `children` as the per-message widgets of `messageId`'s card. */
export function CardScope({
  messageId,
  subject,
  children,
}: {
  messageId: MessageId;
  subject: string;
  children: ReactNode;
}) {
  const parent = useUiStoreApi();
  const store = useMemo(() => scopedUiStore(parent, messageId), [parent, messageId]);
  const card = useMemo(() => ({ messageId, subject }), [messageId, subject]);
  return (
    <UiStoreContext.Provider value={store}>
      <ReaderCardContext.Provider value={card}>{children}</ReaderCardContext.Provider>
    </UiStoreContext.Provider>
  );
}
