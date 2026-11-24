/**
 * The open question of a confirmation window (R6, ADR 0239): what a
 * component asks about is kept per app store until the window that answers
 * it is gone. The component that asks stores it and opens the window
 * (`askInWindow`); the window reads it once when it opens and forgets it
 * however the question ends — the verb, Cancel, × or Esc
 * (`usePendingQuestion`).
 *
 * The key is the app store (`UiStoreMap`): a card of the reader's feed has
 * a scoped view of the app store (`scopedUiStore`) that shares its entry, so
 * the card asks and the window (outside the card) answers.
 */
import { useEffect, useState } from "react";
import { UiStoreMap, type UiStore } from "../../state/uiStore";

/** One kind of open question, per app store. */
export interface PendingSlot<T> {
  /** The open question (`null` = none). */
  get(store: UiStore): T | null;
  set(store: UiStore, what: T): void;
  clear(store: UiStore): void;
  /** Forgets `what` if it is still the open question (a newer one stays). */
  forget(store: UiStore, what: T | null): void;
}

/** A new slot for one kind of question. */
export function pendingSlot<T>(): PendingSlot<T> {
  const pending = new UiStoreMap<T>();
  return {
    get: (store) => pending.get(store) ?? null,
    set: (store, what) => {
      pending.set(store, what);
    },
    clear: (store) => {
      pending.delete(store);
    },
    forget: (store, what) => {
      if (what !== null && pending.get(store) === what) pending.delete(store);
    },
  };
}

/** Keeps `what` as the open question of `slot` and opens the window `window` that asks it. */
export function askInWindow<T>(
  slot: PendingSlot<T>,
  store: UiStore,
  window: string,
  what: T,
): void {
  slot.set(store, what);
  store.getState().openWindow(window);
}

/**
 * The question a confirmation window answers: read once when it opens, and
 * forgotten when the window goes (however the question ends).
 */
export function usePendingQuestion<T>(slot: PendingSlot<T>, store: UiStore): T | null {
  const [what] = useState(() => slot.get(store));
  useEffect(
    () => () => {
      slot.forget(store, what);
    },
    [slot, store, what],
  );
  return what;
}
