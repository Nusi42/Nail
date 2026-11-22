/**
 * The one confirmation (R6, ADR 0239): a question, its detail, a verb
 * button and Cancel, in the shared footer (`./DialogFooter.tsx`). It is
 * the content of a modal confirmation window (`{ confirmation: true }`).
 *
 * - `risk="destructive"`: the verb button is red ("Delete folder"), never
 *   the default — the focus starts on Cancel and Enter never runs it.
 * - `risk="normal"`: the verb button has the focus and Enter runs it
 *   (also from a field in the detail).
 * - A failed attempt (`busy` back to false, the window still open) gives
 *   the focus back to the same button; the error shows inline (`error`).
 * - × and Esc mean Cancel (the window's close).
 *
 * Parts: `dialog.confirm` (`data-confirm` = what is asked, `data-risk`),
 * `dialog.question`, `dialog.detail`, `form.error`, then `dialog.footer`.
 */
import { useEffect, useId, useRef, type KeyboardEvent, type ReactNode } from "react";
import { DialogFooter, type FooterButton, type FooterCancel } from "./DialogFooter";

export type ConfirmRisk = "normal" | "destructive";

export interface ConfirmDialogProps {
  /** What is asked (`data-confirm`; also the verb button's `data-action`). */
  kind: string;
  /** The question, localized ("Delete the folder “Customers”?"). */
  question: string;
  /** Facts below the question (a note, the real target, a count). */
  detail?: ReactNode;
  /** The verb, localized ("Delete folder"); never "OK" or "Yes". */
  action: string;
  onConfirm: () => void;
  /** `destructive`: red, not the default, the focus on Cancel. */
  risk?: ConfirmRisk;
  /** Cancel (the window's close); its label defaults to "Cancel". */
  cancel: FooterCancel;
  /** A second choice between the verb and Cancel ("Don't save"). */
  secondary?: FooterButton;
  /** The verb cannot run (nothing left to act on). */
  canConfirm?: boolean;
  /** The action runs: every button but Cancel is disabled. */
  busy?: boolean;
  /** Why the last attempt failed, localized. */
  error?: string | null;
}

/** Elements whose own Enter must stay theirs. */
const OWN_ENTER = "button, a[href], textarea, select, [contenteditable='true']";

export function ConfirmDialog({
  kind,
  question,
  detail,
  action,
  onConfirm,
  risk = "normal",
  cancel,
  secondary,
  canConfirm = true,
  busy = false,
  error = null,
}: ConfirmDialogProps) {
  const id = useId();
  const root = useRef<HTMLDivElement>(null);
  const wasBusy = useRef(false);
  const destructive = risk === "destructive";
  /** The button that gets the focus: the safe one when the verb is destructive. */
  const focusTarget = destructive ? "cancel" : "primary";
  const focusStart = () => {
    const el = root.current?.querySelector<HTMLButtonElement>(
      `[data-part="dialog.footer"] [data-role="${focusTarget}"]:not(:disabled)`,
    );
    (el ?? root.current?.querySelector<HTMLButtonElement>('[data-role="cancel"]'))?.focus();
  };
  // After the window frame focused its first control.
  useEffect(() => {
    queueMicrotask(focusStart);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- once, when the question opens.
  }, []);
  // A failed attempt: the busy buttons dropped the focus; it goes back.
  useEffect(() => {
    if (busy) {
      wasBusy.current = true;
      return;
    }
    if (!wasBusy.current) return;
    wasBusy.current = false;
    focusStart();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- runs when `busy` changes.
  }, [busy]);
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Enter" || event.altKey || event.ctrlKey || event.metaKey) return;
    if (event.target instanceof Element && event.target.closest(OWN_ENTER)) return;
    event.preventDefault();
    if (!destructive && canConfirm && !busy) onConfirm();
  };
  return (
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- Enter in a field of the detail runs a normal confirmation; the buttons inside take the focus.
    <div
      ref={root}
      data-part="dialog.confirm"
      data-confirm={kind}
      data-risk={risk}
      role="group"
      aria-labelledby={`${id}-question`}
      aria-busy={busy || undefined}
      onKeyDown={onKeyDown}
    >
      <p data-part="dialog.question" id={`${id}-question`}>
        {question}
      </p>
      {detail !== undefined && <div data-part="dialog.detail">{detail}</div>}
      {error !== null && (
        <p data-part="form.error" role="alert">
          {error}
        </p>
      )}
      <DialogFooter
        primary={{
          label: action,
          onClick: onConfirm,
          destructive,
          disabled: !canConfirm,
          action: kind,
        }}
        {...(secondary === undefined ? {} : { secondary })}
        cancel={cancel}
        busy={busy}
      />
    </div>
  );
}

/**
 * A confirmation window opened without a question (it was answered
 * elsewhere, or the window was left over): the text and one Done.
 */
export function ConfirmGone({
  kind,
  text,
  close,
}: {
  kind: string;
  /** Why there is nothing to ask, localized. */
  text: string;
  close: () => void;
}) {
  return (
    <div data-part="dialog.confirm" data-confirm={kind} data-state="empty">
      <p data-part="dialog.question">{text}</p>
      <DialogFooter done={{ onDone: close }} />
    </div>
  );
}
