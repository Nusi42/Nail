/**
 * Undo / Redo of the appearance history (M5d, ADR 0131) for the Appearance
 * page and the theme editor: the buttons name what they undo ("Undo Edit
 * theme"); a theme file changed outside Nail answers `conflict`, and then
 * "Undo anyway" replaces those changes (the core backs the file up first).
 */
import { useState } from "react";
import { errorMessageOf, NailError } from "../../api/client";
import { useT } from "../../i18n/I18nProvider";
import { nextRedo, nextUndo, useEditHistory, useEditHistoryStep } from "../../state/editHistory";
import { Icon } from "../../widgets/Icon";

export function HistoryButtons() {
  const t = useT();
  const history = useEditHistory("appearance");
  const step = useEditHistoryStep("appearance");
  const [conflict, setConflict] = useState<"undo" | "redo" | null>(null);
  const undo = nextUndo(history.data);
  const redo = nextRedo(history.data);
  const run = (direction: "undo" | "redo", force = false) => {
    setConflict(null);
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
  return (
    <div data-part="appearance.history" role="group" aria-label={t("appearance-history-label")}>
      <button
        type="button"
        data-part="form.button"
        data-variant="ghost"
        disabled={!undo || step.isPending}
        aria-label={
          undo
            ? t("appearance-undo-named", { action: t(undo.label.id, undo.label.args) })
            : t("appearance-undo")
        }
        onClick={() => {
          run("undo");
        }}
      >
        <Icon name="undo-2" part="form.button.icon" />
        {t("appearance-undo")}
      </button>
      <button
        type="button"
        data-part="form.button"
        data-variant="ghost"
        disabled={!redo || step.isPending}
        aria-label={
          redo
            ? t("appearance-redo-named", { action: t(redo.label.id, redo.label.args) })
            : t("appearance-redo")
        }
        onClick={() => {
          run("redo");
        }}
      >
        <Icon name="redo-2" part="form.button.icon" />
        {t("appearance-redo")}
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
                run(conflict, true);
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
