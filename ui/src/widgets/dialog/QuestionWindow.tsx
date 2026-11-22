/**
 * Questions before an action (R6, ADR 0239): "Remove account…", "Delete
 * event", "Delete group", "Delete contact" and the like ask in the shared
 * `ConfirmDialog`, in a modal confirmation window of their own kind (title
 * `window-<kind>`), instead of an inline "Are you sure" row. A feature
 * registers its kinds with `QuestionWindow` (`dialogs`) and
 * `{ confirmation: true }` (`windows`), then asks with
 * `askQuestion(ui, kind, question)`.
 *
 * The verb runs `run`: when it succeeds the window closes, when it throws
 * the window stays with the core's reason inline (and the focus back on the
 * button). ×, Esc and Cancel forget the question; a window opened without
 * a question says so and offers Done.
 */
import { useState } from "react";
import { errorMessageOf } from "../../api/client";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";
import { ConfirmDialog, ConfirmGone, type ConfirmRisk } from "./ConfirmDialog";
import { askInWindow, pendingSlot, usePendingQuestion, type PendingSlot } from "./pending";

/** A question asked before an action (texts localized by the component that asks). */
export interface DialogQuestion {
  /** "Remove the account “Work”?" */
  question: string;
  /** Facts below the question. */
  detail?: string;
  /** The verb ("Remove account"); never "OK" or "Yes". */
  action: string;
  /**
   * A check box the user must tick before the verb runs ("Also delete my
   * secret key"), its label localized.
   */
  confirmCheck?: string;
  /** `destructive` (default): red, the focus on Cancel, Enter never runs it. */
  risk?: ConfirmRisk;
  /** Does it; throws (or rejects) when it failed (the reason shows in the window). */
  run: () => unknown;
}

/** One slot per window kind. */
const slots = new Map<string, PendingSlot<DialogQuestion>>();

function slotOf(kind: string): PendingSlot<DialogQuestion> {
  let slot = slots.get(kind);
  if (!slot) {
    slot = pendingSlot<DialogQuestion>();
    slots.set(kind, slot);
  }
  return slot;
}

/** Asks `question` in the confirmation window `kind`. */
export function askQuestion(ui: UiStore, kind: string, question: DialogQuestion): void {
  askInWindow(slotOf(kind), ui, kind, question);
}

/** The open question of window kind `kind` (`null`: none). */
export function questionOf(ui: UiStore, kind: string): DialogQuestion | null {
  return slotOf(kind).get(ui);
}

/** The content of a question window (any registered kind). */
export function QuestionWindow({ name, close }: DialogProps) {
  const t = useT();
  const ui = useUiStoreApi();
  const question = usePendingQuestion(slotOf(name), ui);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [checked, setChecked] = useState(false);
  // Nothing asked (a window left over): only Cancel can do something.
  if (question === null) {
    return <ConfirmGone kind={name} text={t("dialog-question-gone")} close={close} />;
  }
  const confirm = async () => {
    setBusy(true);
    setError(null);
    try {
      await question.run();
    } catch (e) {
      const message = errorMessageOf(e);
      setError(t(message.id, message.args));
      setBusy(false);
      return;
    }
    setBusy(false);
    close();
  };
  const hasDetail = question.detail !== undefined || question.confirmCheck !== undefined;
  return (
    <ConfirmDialog
      kind={name}
      question={question.question}
      {...(hasDetail
        ? {
            detail: (
              <>
                {question.detail !== undefined && <p data-part="dialog.note">{question.detail}</p>}
                {question.confirmCheck !== undefined && (
                  <label data-part="form.check">
                    <input
                      type="checkbox"
                      data-part="form.checkbox"
                      checked={checked}
                      onChange={(event) => {
                        setChecked(event.target.checked);
                      }}
                    />
                    <span data-part="form.check.label">{question.confirmCheck}</span>
                  </label>
                )}
              </>
            ),
          }
        : {})}
      canConfirm={question.confirmCheck === undefined || checked}
      action={question.action}
      risk={question.risk ?? "destructive"}
      busy={busy}
      error={error}
      onConfirm={() => void confirm()}
      cancel={{ onClick: close }}
    />
  );
}
