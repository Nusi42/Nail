/**
 * Mail confirmations: the reader's "Open anyway" (ADR 0206: a flagged
 * link, ADR 0211: a flagged attachment), "Delete permanently" and the move
 * window's question before Drafts, Trash or Junk (ADR 0230). The pending
 * question per app store, and the dialog's body with the safe choice
 * (Cancel) focused, also again after a failed attempt.
 */
import { useEffect, useId, useRef, type ReactNode } from "react";
import { useT } from "../i18n/I18nProvider";
import type { UiStore } from "../state/uiStore";

/**
 * What a confirmation is about, by the app's store. A card of the reader's
 * feed has a scoped view of that store (`scopedUiStore`) that shares its
 * `setState`, so that is the key: the card asks, the window (outside the
 * card) answers.
 */
export interface PendingSlot<T> {
  /** The open question (`null` = none). */
  get(store: UiStore): T | null;
  set(store: UiStore, what: T): void;
  clear(store: UiStore): void;
}

/** A new slot for one kind of confirmation. */
export function pendingSlot<T>(): PendingSlot<T> {
  const pending = new WeakMap<UiStore["setState"], T>();
  return {
    get: (store) => pending.get(store.setState) ?? null,
    set: (store, what) => {
      pending.set(store.setState, what);
    },
    clear: (store) => {
      pending.delete(store.setState);
    },
  };
}

/** The body of a confirmation: the question, its facts, the action and Cancel. */
export function MailConfirm({
  kind,
  question,
  children,
  confirmLabel,
  variant = "danger",
  canConfirm = true,
  busy = false,
  dataRole,
  onConfirm,
  onCancel,
}: {
  /** `data-confirm` and `data-action` of the confirmation. */
  kind: string;
  question: string;
  /** The facts: warnings, the real target, a note. */
  children?: ReactNode;
  /** The action button's text (default: "Open anyway"). */
  confirmLabel?: string;
  /** The action button's `data-variant`. */
  variant?: "danger" | "primary";
  canConfirm?: boolean;
  /** The action runs: both buttons are disabled (`aria-busy`). */
  busy?: boolean;
  /** `data-role` of the confirmation (e.g. the folder role a move goes to). */
  dataRole?: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const t = useT();
  const cancel = useRef<HTMLButtonElement>(null);
  const wasBusy = useRef(false);
  const questionId = useId();
  // The safe choice has the focus (after the frame focused its first button).
  useEffect(() => {
    queueMicrotask(() => cancel.current?.focus());
  }, []);
  // A failed attempt leaves the question open: the action button was
  // disabled while it ran (the browser drops its focus), so the focus goes
  // back to the safe choice. A successful one closes the window.
  useEffect(() => {
    if (busy) {
      wasBusy.current = true;
      return;
    }
    if (!wasBusy.current) return;
    wasBusy.current = false;
    cancel.current?.focus();
  }, [busy]);
  return (
    <div
      data-part="mail-confirm"
      data-confirm={kind}
      data-role={dataRole}
      aria-busy={busy}
      role="group"
      aria-labelledby={questionId}
    >
      <p data-part="mail-confirm.question" id={questionId}>
        {question}
      </p>
      {children}
      <div data-part="mail-confirm.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant={variant}
          data-action={kind}
          disabled={busy || !canConfirm}
          onClick={onConfirm}
        >
          {confirmLabel ?? t("mail-open-link-open")}
        </button>
        <button
          ref={cancel}
          type="button"
          data-part="form.button"
          disabled={busy}
          onClick={onCancel}
        >
          {t("common-cancel")}
        </button>
      </div>
    </div>
  );
}
