/**
 * The rules window's questions (R2 design review, items 4 and 5): delete a
 * rule, discard unsaved changes, run a rule that moves or deletes mail.
 * They are asked inside the app — never a browser dialog — over the rules
 * window's content, which is `inert` meanwhile (the editor's content and,
 * through the store's `asking`, the rule list), so the question is the only
 * thing to answer.
 *
 * The safe choice (Cancel / Keep editing) has the focus; Escape answers it
 * and goes no further (the window's own Escape would close the window).
 *
 * Parts: `rules.confirm` (`data-confirm`: what is asked), `.question`,
 * `.actions`.
 */
import { useEffect, useId, useRef } from "react";

export interface ConfirmProps {
  /** What is asked (`data-confirm`: `delete`, `discard`, `run`). */
  kind: string;
  /** The question, localized. */
  question: string;
  /** The label of the button that does it, localized. */
  confirmLabel: string;
  /** The label of the safe choice, localized. */
  cancelLabel: string;
  /** The action loses something (a destructive button). */
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export function Confirm({
  kind,
  question,
  confirmLabel,
  cancelLabel,
  danger = false,
  onConfirm,
  onCancel,
}: ConfirmProps) {
  const id = useId();
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    cancel.current?.focus();
  }, []);
  return (
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- Escape anywhere in the question answers it with the safe choice (the buttons inside take the focus).
    <div
      data-part="rules.confirm"
      data-confirm={kind}
      role="alertdialog"
      aria-modal="true"
      aria-labelledby={`${id}-question`}
      onKeyDown={(event) => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        event.stopPropagation();
        onCancel();
      }}
    >
      <p data-part="rules.confirm.question" id={`${id}-question`}>
        {question}
      </p>
      <div data-part="rules.confirm.actions">
        <button
          ref={cancel}
          type="button"
          data-part="form.button"
          data-action="confirm-cancel"
          onClick={onCancel}
        >
          {cancelLabel}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-variant={danger ? "danger" : "primary"}
          data-action={`confirm-${kind}`}
          onClick={onConfirm}
        >
          {confirmLabel}
        </button>
      </div>
    </div>
  );
}
