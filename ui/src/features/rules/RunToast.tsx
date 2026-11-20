/**
 * What a run from the "Run rule" menu did (R1): "2 rules ran: 5 messages
 * matched, 5 changes queued." with Undo (every run of the click), or the
 * core's error. The app's shared `Toast` widget shows it (look, timing,
 * Dismiss, one toast at a time); `data-source="rules"` tells it from a mail
 * action's toast.
 */
import { useQueryClient } from "@tanstack/react-query";
import { errorMessageIdOf } from "../../api/client";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { Toast, type ToastKind } from "../../widgets/Toast";
import { refreshRuns } from "./runMenu";
import { outcomeMessage, outcomeOf, undoRuns } from "./runTargets";
import { useRulesUi, type RunToast as RunToastState } from "./store";

function toastText(toast: RunToastState): { id: string; args?: Record<string, number> } {
  switch (toast.kind) {
    case "ran":
      return outcomeMessage(outcomeOf(toast.runs));
    case "undone":
      return { id: "rules-undone-all" };
    case "error":
      return { id: toast.id };
  }
}

/** The shared toast's kind of a run toast. */
export function toastKindOf(toast: RunToastState): ToastKind {
  return toast.kind === "ran" ? "done" : toast.kind;
}

export function RunToast() {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const toast = useRulesUi((s) => s.toast);
  const show = useRulesUi((s) => s.showToast);
  const dismiss = useRulesUi((s) => s.dismissToast);

  if (!toast) return null;
  const text = toastText(toast);
  const undoable = toast.kind === "ran" ? outcomeOf(toast.runs).undoable : [];
  const undo = async () => {
    try {
      await undoRuns(client, undoable);
      show({ kind: "undone" });
      await refreshRuns(queryClient);
    } catch (error) {
      show({ kind: "error", id: errorMessageIdOf(error) });
    }
  };
  return (
    <Toast
      kind={toastKindOf(toast)}
      seq={toast.seq}
      text={t(text.id, text.args)}
      source="rules"
      onDismiss={dismiss}
      undo={
        undoable.length > 0 ? { label: t("rules-undo-all"), run: () => void undo() } : undefined
      }
    />
  );
}
