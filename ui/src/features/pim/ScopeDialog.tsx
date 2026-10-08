/**
 * "This occurrence / this and following / all occurrences" (dialog
 * `pim-edit-scope`) for a change of a recurring event made in a view: a
 * dragged or resized occurrence, or a deleted one (`PimState.pendingChange`).
 */
import { useId, useState, type SubmitEvent } from "react";
import type { EditScope, LocalizedMessage } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { applyDelete, applyMove } from "./changes";
import { Problem, problemOf, useChangeContext } from "./common";
import { EDIT_SCOPES } from "./constants";
import { formatPimTime } from "./fmt";
import { usePim } from "./store";
import { localZone } from "./time";
import { EmptyDialog } from "../../widgets/DialogStates";

export function ScopeDialog({ close }: DialogProps) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useChangeContext();
  const change = usePim((s) => s.pendingChange);
  const [scope, setScope] = useState<EditScope>("this");
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  const name = useId();

  const finish = () => {
    ctx.pim.getState().setPendingChange(null);
    close();
  };

  if (change === null) {
    return (
      <EmptyDialog
        part="pim.scope"
        textPart="pim.scope.empty"
        hintId="pim-scope-nothing"
        close={finish}
      />
    );
  }

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    setBusy(true);
    setProblem(null);
    try {
      if (change.kind === "move")
        await applyMove(ctx, change.instance, change.start, change.end, scope);
      else await applyDelete(ctx, change.instance, scope);
      finish();
    } catch (error) {
      setProblem(problemOf(error));
    } finally {
      setBusy(false);
    }
  };

  const tz = localZone();
  return (
    <form
      data-part="pim.scope"
      data-kind={change.kind}
      onSubmit={(e) => void submit(e)}
      aria-busy={busy}
    >
      <p data-part="pim.scope.intro">
        {change.kind === "move"
          ? t("pim-scope-move-intro", {
              title: change.instance.title || t("common-no-title"),
              time: formatPimTime(format, change.start, tz),
            })
          : t("pim-scope-delete-intro", {
              title: change.instance.title || t("common-no-title"),
            })}
      </p>
      <fieldset data-part="pim.scope.choices">
        <legend data-part="form.section.title">{t("pim-scope-legend")}</legend>
        {EDIT_SCOPES.map((value) => (
          <label key={value} data-part="form.check" data-scope={value}>
            <input
              type="radio"
              name={name}
              value={value}
              checked={scope === value}
              onChange={() => {
                setScope(value);
              }}
            />
            <span>{t(`pim-scope-${value}`)}</span>
          </label>
        ))}
      </fieldset>
      <Problem message={problem} />
      <div data-part="pim.scope.actions">
        <button
          type="submit"
          data-part="form.button"
          data-variant={change.kind === "delete" ? "danger" : "primary"}
          disabled={busy}
        >
          {change.kind === "move" ? t("common-apply") : t("pim-delete")}
        </button>
        <button type="button" data-part="form.button" onClick={finish}>
          {t("common-cancel")}
        </button>
      </div>
    </form>
  );
}
