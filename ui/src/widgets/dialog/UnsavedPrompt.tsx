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
 */
import { useEffect, useRef, useState } from "react";
import { useT } from "../../i18n/I18nProvider";
import {
  MAIN_GUARD,
  closeGuardOf,
  forgetUnsavedQuestion,
  guardedWindowOf,
  unsavedQuestionOf,
} from "../../layout/closeGuard";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useUiStoreApi } from "../../state/uiStore";
import { nonEmpty } from "../../text/empty";
import { ConfirmDialog, ConfirmGone } from "./ConfirmDialog";

/** Window `unsaved-changes:<key>`. */
export function UnsavedPrompt({ name, close }: DialogProps) {
  const t = useT();
  const ui = useUiStoreApi();
  const target = guardedWindowOf(name) ?? MAIN_GUARD;
  const [question] = useState(() => unsavedQuestionOf(ui, target));
  const guard = closeGuardOf(ui, target);
  const [busy, setBusy] = useState(false);
  /** Cancel (or ×, Esc) was chosen: a save still running must not move on. */
  const cancelled = useRef(false);
  useEffect(
    () => () => {
      if (question) forgetUnsavedQuestion(ui, question);
    },
    [ui, question],
  );
  if (question === null || guard === null) {
    return <ConfirmGone kind="unsaved" text={t("dialog-unsaved-gone")} close={close} />;
  }
  /** Back to the window, which keeps its changes. */
  const keepEditing = () => {
    cancelled.current = true;
    close();
    if (target !== MAIN_GUARD) ui.getState().focusWindow(target);
  };
  /** The answer lets the window go (or move on). */
  const goOn = () => {
    close();
    question.proceed();
  };
  const dropChanges = () => {
    guard.discard?.();
    goOn();
  };
  const itemName = nonEmpty(guard.name().trim()) ?? t("dialog-untitled");
  if (guard.canSave?.() !== true) {
    return (
      <ConfirmDialog
        kind="unsaved"
        risk="destructive"
        question={t("dialog-unsaved-invalid-question", { name: itemName })}
        detail={<p data-part="dialog.note">{t("dialog-unsaved-invalid-detail")}</p>}
        action={t("common-discard-changes")}
        onConfirm={dropChanges}
        cancel={{ label: t("common-keep-editing"), onClick: keepEditing }}
      />
    );
  }
  const save = async () => {
    setBusy(true);
    const saved = await Promise.resolve()
      .then(() => guard.save?.() ?? false)
      .catch(() => false);
    setBusy(false);
    // Cancelled while saving: the window stays (saved or not).
    if (cancelled.current) return;
    if (saved) goOn();
    else keepEditing();
  };
  return (
    <ConfirmDialog
      kind="unsaved"
      question={t("dialog-unsaved-question", { name: itemName })}
      detail={
        <p data-part="dialog.note">
          {nonEmpty(guard.detail?.() ?? "") ?? t("dialog-unsaved-detail")}
        </p>
      }
      action={nonEmpty(guard.saveLabel?.() ?? "") ?? t("common-save")}
      onConfirm={() => void save()}
      secondary={{
        label: t("dialog-dont-save"),
        destructive: true,
        onClick: dropChanges,
        action: "dont-save",
      }}
      cancel={{ onClick: keepEditing }}
      busy={busy}
    />
  );
}
