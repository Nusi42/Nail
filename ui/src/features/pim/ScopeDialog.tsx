/**
 * "This occurrence / this and following / all occurrences" (dialog
 * `pim-edit-scope`) for a change of a recurring event made in a view: a
 * dragged or resized occurrence, or a deleted one (`PimState.pendingChange`).
 *
 * R6 (ADR 0239): the verb names the operation. A move is a form whose
 * "Save changes" Enter runs; a delete is a destructive `ConfirmDialog`
 * ("Delete events", the focus on Cancel, Enter never deleting). ×, Esc and
 * Cancel drop the change.
 */
import { useEffect, useState } from "react";
import type { EditScope, LocalizedMessage } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { applyDelete, applyMove } from "./changes";
import { Problem, problemOf, ScopeRadios, useChangeContext } from "./common";
import { EDIT_SCOPES } from "./constants";
import { formatPimTime } from "./fmt";
import { usePim } from "./store";
import { localZone } from "./time";
import { EmptyDialog } from "../../widgets/DialogStates";
import { ConfirmDialog, DialogFooter, DialogForm } from "../../widgets/dialog";

export function ScopeDialog({ close }: DialogProps) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useChangeContext();
  const change = usePim((s) => s.pendingChange);
  const [scope, setScope] = useState<EditScope>("this");
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  // However the question ends (Cancel, ×, Esc, done), the change is forgotten.
  useEffect(
    () => () => {
      if (ctx.pim.getState().pendingChange === change) ctx.pim.getState().setPendingChange(null);
    },
    [ctx.pim, change],
  );

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

  const submit = async () => {
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
  const title = change.instance.title || t("common-no-title");
  const choices = (
    <ScopeRadios
      part="pim.scope.choices"
      legend={t("pim-scope-legend")}
      values={EDIT_SCOPES}
      value={scope}
      label={(value) => t(`pim-scope-${value}`)}
      onChange={setScope}
    />
  );
  if (change.kind === "delete") {
    return (
      <ConfirmDialog
        kind="pim-scope-delete"
        risk="destructive"
        question={t("pim-scope-delete-intro", { title })}
        detail={choices}
        action={t("pim-scope-delete")}
        busy={busy}
        error={problem === null ? null : t(problem.id, problem.args)}
        onConfirm={() => void submit()}
        cancel={{ onClick: finish }}
      />
    );
  }
  return (
    <DialogForm part="pim.scope" dialog={change.kind} busy={busy} onSubmit={() => void submit()}>
      <p data-part="pim.scope.intro">
        {t("pim-scope-move-intro", { title, time: formatPimTime(format, change.start, tz) })}
      </p>
      {choices}
      <Problem message={problem} />
      <DialogFooter
        primary={{ label: t("pim-scope-save"), submit: true, action: "move" }}
        cancel={{ onClick: finish }}
        busy={busy}
      />
    </DialogForm>
  );
}
