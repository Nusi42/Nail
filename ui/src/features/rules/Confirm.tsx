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
 * Tab stays inside the question (it is modal). Once answered, the focus
 * goes back to what had it when the question came up (the button that
 * asked, the rule picked in the list, the field Escape was pressed in);
 * when that is gone (a deleted rule's editor), to the first control of the
 * rules window.
 *
 * Parts: `rules.confirm` (`data-confirm`: what is asked), `.question`,
 * `.actions`.
 */
import { useEffect, useId, useRef, useState } from "react";
import { tabbables, trapTab } from "../../layout/WindowLayer";

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
  const root = useRef<HTMLDivElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  // Read while rendering: once the question shows, the content behind it is
  // `inert` and has lost the focus.
  const [opener] = useState(() => {
    const active = typeof document === "undefined" ? null : document.activeElement;
    return active instanceof HTMLElement && active !== document.body ? active : null;
  });
  useEffect(() => {
    const window = root.current?.closest<HTMLElement>('[data-part="rules"]') ?? null;
    cancel.current?.focus();
    return () => {
      // After the answer has taken the rule list's `inert` away too.
      setTimeout(() => {
        restoreFocus(opener, window);
      }, 0);
    };
  }, [opener]);
  return (
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- Escape anywhere in the question answers it with the safe choice; Tab stays inside it (the buttons inside take the focus).
    <div
      ref={root}
      data-part="rules.confirm"
      data-confirm={kind}
      role="alertdialog"
      aria-modal="true"
      aria-labelledby={`${id}-question`}
      onKeyDown={(event) => {
        if (event.key === "Tab") {
          trapTab(event, event.currentTarget);
          if (event.defaultPrevented) event.stopPropagation();
          return;
        }
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

/**
 * Gives the focus back to `opener` after a question, or — when it is gone,
 * hidden or `inert` — to the first control of the rules window `window`.
 * Leaves it alone when something else took it meanwhile or the window is
 * gone (it closed).
 */
export function restoreFocus(opener: HTMLElement | null, window: HTMLElement | null): void {
  const active = document.activeElement;
  if (active !== null && active !== document.body) return;
  if (opener?.isConnected && opener.closest("[inert], [hidden]") === null) {
    opener.focus();
    return;
  }
  if (window?.isConnected) tabbables(window)[0]?.focus();
}
