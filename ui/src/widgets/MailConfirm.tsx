/**
 * The reader's "Open anyway" confirmations (ADR 0206: a flagged link, ADR
 * 0211: a flagged attachment): the pending question per app store, and the
 * modal dialog's body with the safe choice (Cancel) focused.
 */
import { useEffect, useRef, type ReactNode } from "react";
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

/** The dialog body of an "Open anyway" / "Cancel" confirmation. */
export function MailConfirm({
  kind,
  question,
  children,
  canOpen,
  onOpen,
  onCancel,
}: {
  /** `data-confirm` and `data-action` of the confirmation. */
  kind: string;
  question: string;
  /** The facts: warnings, the real target. */
  children: ReactNode;
  canOpen: boolean;
  onOpen: () => void;
  onCancel: () => void;
}) {
  const t = useT();
  const cancel = useRef<HTMLButtonElement>(null);
  // The safe choice has the focus (after the frame focused its first button).
  useEffect(() => {
    queueMicrotask(() => cancel.current?.focus());
  }, []);
  return (
    <div data-part="mail-confirm" data-confirm={kind}>
      <p data-part="mail-confirm.question">{question}</p>
      {children}
      <div data-part="mail-confirm.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant="danger"
          data-action={kind}
          disabled={!canOpen}
          onClick={onOpen}
        >
          {t("mail-open-link-open")}
        </button>
        <button ref={cancel} type="button" data-part="form.button" onClick={onCancel}>
          {t("common-cancel")}
        </button>
      </div>
    </div>
  );
}
