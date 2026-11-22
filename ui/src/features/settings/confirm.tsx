/**
 * Questions of the settings sections (R6, ADR 0239): "Remove account…",
 * "Delete virtual account", "Reset the classifier", "Revoke" and the like
 * ask in the shared `ConfirmDialog`, in a modal confirmation window of
 * their own kind (title `window-<kind>`), instead of an inline "Are you
 * sure" row. A feature registers its kinds with `SettingsConfirmDialog`
 * (`dialogs`) and `{ confirmation: true }` (`windows`), then asks with
 * `askSettingsQuestion(ui, kind, question)`.
 *
 * The verb runs `run`: when it succeeds the window closes, when it throws
 * the window stays with the error inline (and the focus back on the
 * verb). ×, Esc and Cancel forget the question.
 */
import { useEffect, useState } from "react";
import { errorMessageOf } from "../../api/client";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { windowTitleId } from "../../layout/WindowLayer";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";
import { ConfirmDialog, type ConfirmRisk } from "../../widgets/dialog";

/** A question asked before a settings action (texts localized). */
export interface SettingsQuestion {
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
  /** Does it; throws when it failed (the error shows in the window). */
  run: () => unknown;
}

const asked = new WeakMap<UiStore, Map<string, SettingsQuestion>>();

function questionsOf(ui: UiStore): Map<string, SettingsQuestion> {
  let questions = asked.get(ui);
  if (!questions) {
    questions = new Map();
    asked.set(ui, questions);
  }
  return questions;
}

/** The open question of window kind `kind`, if any. */
export function settingsQuestionOf(ui: UiStore, kind: string): SettingsQuestion | null {
  return asked.get(ui)?.get(kind) ?? null;
}

/** Asks `question` in the confirmation window `kind`. */
export function askSettingsQuestion(ui: UiStore, kind: string, question: SettingsQuestion): void {
  questionsOf(ui).set(kind, question);
  ui.getState().openWindow(kind);
}

/** The content of a settings confirmation window (any registered kind). */
export function SettingsConfirmDialog({ name, close }: DialogProps) {
  const t = useT();
  const ui = useUiStoreApi();
  const [question] = useState(() => settingsQuestionOf(ui, name));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [checked, setChecked] = useState(false);
  // However the window closes, the question is forgotten.
  useEffect(
    () => () => {
      if (question !== null && asked.get(ui)?.get(name) === question) asked.get(ui)?.delete(name);
    },
    [ui, name, question],
  );
  // Nothing asked (a window left over): only Cancel can do something.
  if (question === null) {
    const title = t(windowTitleId(name));
    return (
      <ConfirmDialog
        kind={name}
        question={title}
        action={title}
        canConfirm={false}
        onConfirm={close}
        cancel={{ onClick: close }}
      />
    );
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
  return (
    <ConfirmDialog
      kind={name}
      question={question.question}
      {...(question.detail === undefined && question.confirmCheck === undefined
        ? {}
        : {
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
          })}
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
