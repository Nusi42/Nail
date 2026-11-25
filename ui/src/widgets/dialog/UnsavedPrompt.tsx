/**
 * The shared unsaved-changes prompt (R6, ADR 0239): the window
 * `unsaved-changes:<key>` that the close guard (`../../layout/closeGuard.ts`)
 * opens over a window with unsaved changes when it is closed (×, Esc,
 * Cancel) or left (another settings section).
 *
 * - "Save changes to “X”?" — Save (the guard's verb: "Save draft"; Enter),
 *   Don't save (red), Cancel (× and Esc too: back to the window).
 * - The changes cannot be saved (invalid): "Discard changes to “X”?" —
 *   Discard changes (red, never the default), Keep editing (focused).
 *
 * Save that fails leaves the window open with its inline error; Cancel
 * while Save runs keeps the window too (it does not close or move on when
 * the save ends).
 *
 * Several windows at once (`SEVERAL_GUARD`, R8): "Save changes to 3
 * items?" with their names listed (`dialog.list`); Save saves each in turn
 * and stops at the first that fails (that window comes to the front with
 * its error), Don't save drops all; when one of them cannot be saved, only
 * Discard all and Keep editing.
 */
import { useEffect, useRef, useState } from "react";
import { useT } from "../../i18n/I18nProvider";
import {
  MAIN_GUARD,
  SEVERAL_GUARD,
  type CloseGuard,
  closeGuardOf,
  forgetUnsavedQuestion,
  guardedWindowOf,
  unsavedQuestionOf,
} from "../../layout/closeGuard";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useUiStoreApi } from "../../state/uiStore";
import { nonEmpty } from "../../text/empty";
import { ConfirmDialog, ConfirmGone } from "./ConfirmDialog";

/** A window the prompt asks about, with its close guard. */
interface Guarded {
  window: string;
  guard: CloseGuard;
}

/**
 * Window `unsaved-changes:<key>`: one prompt over the guarded windows it
 * asks about — one window is the list of one (its own texts: its name, the
 * guard's verb and detail), several are named in a list.
 */
export function UnsavedPrompt({ name, close }: DialogProps) {
  const t = useT();
  const ui = useUiStoreApi();
  const target = guardedWindowOf(name) ?? MAIN_GUARD;
  const several = target === SEVERAL_GUARD;
  const question = useQuestion(target);
  const [busy, setBusy] = useState(false);
  /** Cancel (or ×, Esc) was chosen: a save still running must not move on. */
  const cancelled = useRef(false);
  const guarded: Guarded[] = several
    ? (question?.several ?? []).flatMap((window) => {
        const guard = closeGuardOf(ui, window);
        return guard?.isDirty() === true ? [{ window, guard }] : [];
      })
    : [closeGuardOf(ui, target)].flatMap((guard) => (guard ? [{ window: target, guard }] : []));
  const first = guarded[0];
  if (question === null || first === undefined) {
    return <ConfirmGone kind="unsaved" text={t("dialog-unsaved-gone")} close={close} />;
  }
  /** Back to a window (the first by default), which keeps its changes. */
  const keepEditing = (window = first.window) => {
    cancelled.current = true;
    close();
    if (window !== MAIN_GUARD) ui.getState().focusWindow(window);
  };
  /** The answer lets the windows go (or move on). */
  const goOn = () => {
    close();
    question.proceed();
  };
  const dropChanges = () => {
    for (const { guard } of guarded) guard.discard?.();
    goOn();
  };
  const itemName = nonEmpty(first.guard.name().trim()) ?? t("dialog-untitled");
  const names = several && (
    <ul data-part="dialog.list">
      {guarded.map(({ window, guard }) => (
        <li key={window} data-part="dialog.list.item">
          {nonEmpty(guard.name().trim()) ?? t("dialog-untitled")}
        </li>
      ))}
    </ul>
  );
  const note = (text: string) => (
    <>
      {names}
      <p data-part="dialog.note">{text}</p>
    </>
  );
  if (guarded.some(({ guard }) => guard.canSave?.() !== true)) {
    return (
      <ConfirmDialog
        kind="unsaved"
        risk="destructive"
        question={
          several
            ? t("dialog-unsaved-many-invalid-question", { count: guarded.length })
            : t("dialog-unsaved-invalid-question", { name: itemName })
        }
        detail={note(t("dialog-unsaved-invalid-detail"))}
        action={t("common-discard-changes")}
        onConfirm={dropChanges}
        cancel={{
          label: t("common-keep-editing"),
          onClick: () => {
            keepEditing();
          },
        }}
      />
    );
  }
  /** Saves each window in turn; the first that fails comes to the front with its error. */
  const save = async () => {
    setBusy(true);
    for (const { window, guard } of guarded) {
      const saved = await Promise.resolve()
        .then(() => guard.save?.() ?? false)
        .catch(() => false);
      // Cancelled while saving: the windows stay (saved or not).
      if (cancelled.current) return;
      if (!saved) {
        setBusy(false);
        keepEditing(window);
        return;
      }
    }
    setBusy(false);
    goOn();
  };
  return (
    <ConfirmDialog
      kind="unsaved"
      question={
        several
          ? t("dialog-unsaved-many-question", { count: guarded.length })
          : t("dialog-unsaved-question", { name: itemName })
      }
      detail={note(
        (several ? null : nonEmpty(first.guard.detail?.() ?? "")) ?? t("dialog-unsaved-detail"),
      )}
      action={(several ? null : nonEmpty(first.guard.saveLabel?.() ?? "")) ?? t("common-save")}
      onConfirm={() => void save()}
      secondary={{
        label: t("dialog-dont-save"),
        destructive: true,
        onClick: dropChanges,
        action: "dont-save",
      }}
      cancel={{
        onClick: () => {
          keepEditing();
        },
      }}
      busy={busy}
    />
  );
}

/** The answer's state: forgets the question when the prompt goes. */
function useQuestion(target: string) {
  const ui = useUiStoreApi();
  const [question] = useState(() => unsavedQuestionOf(ui, target));
  useEffect(
    () => () => {
      if (question) forgetUnsavedQuestion(ui, question);
    },
    [ui, question],
  );
  return question;
}
