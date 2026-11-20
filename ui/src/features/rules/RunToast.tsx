/**
 * What a run from the "Run rule" menu did (R1): "2 rules ran: 5 messages
 * matched, 5 changes queued." with Undo (every run of the click) and
 * Dismiss, or the core's error. A manual popover (top layer, above every
 * window); it hides after `RUN_TOAST_MS` unless the pointer or the focus is
 * on it.
 *
 * It wears the mail actions' toast parts (`mail-actions.toast`, `.text`,
 * `.undo`, `.dismiss`; `MAIL_ACTIONS_SKIN_CSS`), so the app has one toast
 * look; `data-kind` is `done` / `undone` / `error` as there, and
 * `data-source="rules"` tells the two apart. Undo and Dismiss are both
 * text buttons (`.undo`).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useLayoutEffect, useRef } from "react";
import { errorMessageIdOf } from "../../api/client";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { refreshRuns } from "./runMenu";
import { outcomeMessage, outcomeOf, undoRuns } from "./runTargets";
import { useRulesUi, type RunToast as Toast } from "./store";

/** How long the toast stays (ms) while nobody points at it. */
export const RUN_TOAST_MS = 10_000;

function toastText(toast: Toast): { id: string; args?: Record<string, number> } {
  switch (toast.kind) {
    case "ran":
      return outcomeMessage(outcomeOf(toast.runs));
    case "undone":
      return { id: "rules-undone-all" };
    case "error":
      return { id: toast.id };
  }
}

export function RunToast() {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const toast = useRulesUi((s) => s.toast);
  const show = useRulesUi((s) => s.showToast);
  const dismiss = useRulesUi((s) => s.dismissToast);
  const ref = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const element = ref.current;
    // Test DOMs without popovers show it in place.
    if (toast && element && "showPopover" in element && !element.matches(":popover-open")) {
      element.showPopover();
    }
  }, [toast]);

  useEffect(() => {
    if (!toast) return;
    let timer = 0;
    const hide = () => {
      if (ref.current?.matches(":hover, :focus-within")) {
        timer = window.setTimeout(hide, RUN_TOAST_MS);
      } else {
        dismiss();
      }
    };
    timer = window.setTimeout(hide, RUN_TOAST_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [toast, dismiss]);

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
    <div
      ref={ref}
      key={toast.seq}
      data-part="mail-actions.toast"
      data-source="rules"
      data-kind={toast.kind === "ran" ? "done" : toast.kind}
      popover="manual"
      role={toast.kind === "error" ? "alert" : "status"}
    >
      <span data-part="mail-actions.toast.text">{t(text.id, text.args)}</span>
      {undoable.length > 0 && (
        <button type="button" data-part="mail-actions.toast.undo" onClick={() => void undo()}>
          {t("rules-undo-all")}
        </button>
      )}
      <button
        type="button"
        data-part="mail-actions.toast.undo"
        data-action="dismiss"
        onClick={dismiss}
      >
        {t("common-dismiss")}
      </button>
    </div>
  );
}
