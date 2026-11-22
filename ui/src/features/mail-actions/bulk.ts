/**
 * The confirmation of an action on a whole list (R5, ADR 0235).
 *
 * After Ctrl+A the selection is "the list except these rows", most of them
 * not loaded and never seen, so every action on it asks once, with the
 * true count the core resolved (`ResolveMailTargets`: messages and
 * conversations, no ids), like Gmail's "Confirm bulk action". Confirming
 * applies the same list target with the answer's `as_of` mark, so mail
 * that arrived while the question was open is never touched unasked.
 * Actions on rows picked one by one keep asking only where they did before
 * (Delete permanently, a move into Drafts, Trash or Junk).
 */
import type { MailAction, MailTargetResolution } from "../../api/generated";
import type { UiStore } from "../../state/uiStore";
import { pendingSlot } from "../../widgets/dialog";

/** The confirmation's window kind (dialog name). */
export const BULK_CONFIRM_DIALOG = "mail-bulk-confirm";

/** The open question. */
export interface BulkQuestion {
  action: MailAction;
  resolution: MailTargetResolution;
}

interface Pending extends BulkQuestion {
  answer: (confirmed: boolean) => void;
}

const slot = pendingSlot<Pending>();

/**
 * What the button of the question says (`mail-bulk-confirm`): the action's
 * type, and for the actions that switch something on or off which way.
 */
export function bulkLabelArgs(action: MailAction): {
  action: MailAction["type"];
  on: "yes" | "no";
} {
  const on =
    action.type === "mark-read"
      ? action.read
      : action.type === "flag"
        ? action.flagged
        : action.type === "junk"
          ? action.junk
          : true;
  return { action: action.type, on: on ? "yes" : "no" };
}

/** The open question (`null` = none). */
export function pendingBulk(store: UiStore): BulkQuestion | null {
  return slot.get(store);
}

/**
 * Asks whether `action` may act on what `resolution` counted; `true` once
 * confirmed, `false` when cancelled or the window closed otherwise (Esc,
 * its close button). A new question cancels an open one.
 */
export function confirmBulk(
  store: UiStore,
  action: MailAction,
  resolution: MailTargetResolution,
): Promise<boolean> {
  slot.get(store)?.answer(false);
  return new Promise((resolve) => {
    let open = false;
    const finish = (confirmed: boolean) => {
      unsubscribe();
      if (slot.get(store)?.answer === finish) slot.clear(store);
      resolve(confirmed);
    };
    // Closed without an answer (Esc, the close button): cancelled.
    const unsubscribe = store.subscribe((state) => {
      const shown = state.windows.includes(BULK_CONFIRM_DIALOG);
      if (shown) open = true;
      else if (open) finish(false);
    });
    slot.set(store, { action, resolution, answer: finish });
    store.getState().openWindow(BULK_CONFIRM_DIALOG);
  });
}

/** Answers the open question and closes its window. */
export function answerBulk(store: UiStore, confirmed: boolean): void {
  const pending = slot.get(store);
  slot.clear(store);
  pending?.answer(confirmed);
  store.getState().closeWindow(BULK_CONFIRM_DIALOG);
}
