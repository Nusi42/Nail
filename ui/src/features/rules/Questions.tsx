/**
 * The rule editor's questions (R2 design review, item 5; R6, ADR 0239):
 * deleting a rule and running rules that move or delete mail ask in a
 * modal confirmation window over the rules window — the shared
 * `ConfirmDialog` with a destructive verb, the focus on Cancel and Enter
 * never running it. ×, Esc and Cancel forget the question.
 */
import { useEffect, useState } from "react";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { ConfirmDialog } from "../../widgets/dialog";
import { useRulesUi } from "./store";

/** Forgets the question however the window ends. */
function useQuestion() {
  const question = useRulesUi((s) => s.question);
  const setQuestion = useRulesUi((s) => s.setQuestion);
  const [asked] = useState(question);
  useEffect(
    () => () => {
      setQuestion(null);
    },
    [setQuestion],
  );
  return asked;
}

/** Window `rule-delete`. */
export function RuleDeleteDialog({ close }: DialogProps) {
  const t = useT();
  const question = useQuestion();
  const name = question?.kind === "delete" ? question.name : "";
  return (
    <ConfirmDialog
      kind="delete-rule"
      risk="destructive"
      question={t("rules-confirm-delete", { name })}
      action={t("rules-delete-confirm")}
      canConfirm={question?.kind === "delete"}
      onConfirm={() => {
        close();
        question?.go();
      }}
      cancel={{ onClick: close }}
    />
  );
}

/** Window `rule-run`. */
export function RuleRunDialog({ close }: DialogProps) {
  const t = useT();
  const question = useQuestion();
  const run = question?.kind === "run" ? question : null;
  return (
    <ConfirmDialog
      kind="run-rules"
      risk="destructive"
      question={
        run === null
          ? ""
          : run.names.length === 1
            ? t("rules-confirm-run", {
                name: run.names[0] ?? "",
                count: run.count,
                target: run.target,
              })
            : t("rules-confirm-run-many", {
                rules: run.names.length,
                count: run.count,
                target: run.target,
              })
      }
      action={run?.action ?? t("rules-run")}
      canConfirm={run !== null}
      onConfirm={() => {
        close();
        run?.go();
      }}
      cancel={{ onClick: close }}
    />
  );
}
