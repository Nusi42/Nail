/**
 * Undo / Redo of the appearance history (M5d, ADR 0131) for the Appearance
 * page and the theme editor: the buttons name what they undo ("Undo Edit
 * theme"); a theme file changed outside Nail answers `conflict`, and then
 * "Undo anyway" replaces those changes (the core backs the file up first).
 * Changes still waiting for their delayed write are written first, so Undo
 * undoes them instead of being overtaken by them (ADR 0138). Ctrl+Z /
 * Ctrl+Y (Ctrl+Shift+Z) undo / redo while the focus is in the buttons'
 * region (the Appearance page, the theme editor) but not in a text field,
 * which keeps its own text undo — a way back that needs no readable button.
 */
import { useEffect, useRef, useState } from "react";
import { errorMessageOf, NailError } from "../../api/client";
import { useT } from "../../i18n/I18nProvider";
import {
  historyKey,
  isTextField,
  nextRedo,
  nextUndo,
  useEditHistory,
  useEditHistoryStep,
} from "../../state/editHistory";
import { Icon } from "../../widgets/Icon";
import { flushPendingWrites } from "./pendingWrites";

export function HistoryButtons() {
  const t = useT();
  const history = useEditHistory("appearance");
  const step = useEditHistoryStep("appearance");
  const [conflict, setConflict] = useState<"undo" | "redo" | null>(null);
  const [flushing, setFlushing] = useState(false);
  const undo = nextUndo(history.data);
  const redo = nextRedo(history.data);
  const busy = step.isPending || flushing;
  const group = useRef<HTMLDivElement>(null);
  const run = async (direction: "undo" | "redo", force = false) => {
    setConflict(null);
    setFlushing(true);
    try {
      await flushPendingWrites();
    } finally {
      setFlushing(false);
    }
    step.mutate(
      { direction, force },
      {
        onError: (error) => {
          if (error instanceof NailError && error.code === "conflict") setConflict(direction);
        },
      },
    );
  };
  const failure = step.error;
  // Ctrl+Z / Ctrl+Y in the region: the latest state, read by one listener.
  const latest = useRef({ run, undo, redo, busy });
  useEffect(() => {
    latest.current = { run, undo, redo, busy };
  });
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const direction = historyKey(event);
      if (direction === null || isTextField(event.target)) return;
      const region = group.current?.closest('[role="region"]');
      if (!region || !(event.target instanceof Node) || !region.contains(event.target)) return;
      const now = latest.current;
      if (now.busy || (direction === "undo" ? !now.undo : !now.redo)) return;
      event.preventDefault();
      void now.run(direction);
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
    };
  }, []);
  return (
    <div
      ref={group}
      data-part="appearance.history"
      role="group"
      aria-label={t("appearance-history-label")}
    >
      <button
        type="button"
        data-part="form.button"
        data-variant="ghost"
        disabled={!undo || busy}
        aria-label={
          undo
            ? t("appearance-undo-named", { action: t(undo.label.id, undo.label.args) })
            : t("common-undo")
        }
        onClick={() => {
          void run("undo");
        }}
      >
        <Icon name="undo-2" part="form.button.icon" />
        {t("common-undo")}
      </button>
      <button
        type="button"
        data-part="form.button"
        data-variant="ghost"
        disabled={!redo || busy}
        aria-label={
          redo
            ? t("appearance-redo-named", { action: t(redo.label.id, redo.label.args) })
            : t("common-redo")
        }
        onClick={() => {
          void run("redo");
        }}
      >
        <Icon name="redo-2" part="form.button.icon" />
        {t("common-redo")}
      </button>
      {failure && (
        <p data-part="form.error" role="alert">
          {t(errorMessageOf(failure).id, errorMessageOf(failure).args)}
          {conflict && (
            <button
              type="button"
              data-part="form.button"
              data-variant="danger"
              onClick={() => {
                void run(conflict, true);
              }}
            >
              {t(conflict === "undo" ? "appearance-undo-anyway" : "appearance-redo-anyway")}
            </button>
          )}
        </p>
      )}
    </div>
  );
}
