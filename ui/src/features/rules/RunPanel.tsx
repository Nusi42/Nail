/**
 * Running rules from the rules window (R1, ADR 0219): the rules checked in
 * the list (else the rule being edited) on a target — each rule's own
 * messages (its folders, else the Inboxes; what "Run now" always did), the
 * folder the list shows, or the mail selected in the list (with the count;
 * unavailable, with the reason, while nothing is selected). A rule just
 * written is applied at once, without leaving the window.
 *
 * The outcome line (matched messages, changes queued) is always there, so
 * it never moves anything; Undo takes back every run of the last click.
 * Errors are the compact `Issues` indicator in the button row.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { currentTargets, useListSelection } from "../../actions/mailTargets";
import type { Rule, RuleId } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useBusyAction } from "../../state/busyAction";
import { useClient, useFolders } from "../../state/queries";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { folderTitle } from "../../widgets/ListHeader";
import { RadioGroup, type Choice } from "../portability/Fields";
import { runRules } from "./api";
import { Issues } from "./Issues";
import { refreshRuns } from "./runMenu";
import {
  outcomeMessage,
  outcomeOf,
  runTargetOf,
  undoRuns,
  type RunOutcome,
  type RunTargetKind,
} from "./runTargets";
import { useRulesUi } from "./store";

/** The rules a click runs: the checked ones, else the saved rule being edited. */
export function rulesToRun(picked: readonly RuleId[], current: RuleId | null): RuleId[] {
  if (picked.length > 0) return [...picked];
  return current === null ? [] : [current];
}

/** What the panel says about the rules it runs (Fluent id and arguments). */
function whichText(
  ids: readonly RuleId[],
  picked: readonly RuleId[],
  rules: readonly Rule[],
): { id: string; args?: Record<string, string | number> } {
  if (ids.length === 0) return { id: "rules-run-which-none" };
  if (picked.length > 0) return { id: "rules-run-which-checked", args: { count: ids.length } };
  return {
    id: "rules-run-which-current",
    args: { name: rules.find((r) => r.id === ids[0])?.name ?? "" },
  };
}

/** A target choice; `why` says why it is not available (`null` = it is). */
function choice(value: RunTargetKind, label: string, why: string | null): Choice<RunTargetKind> {
  return why === null ? { value, label } : { value, label, disabled: true, hint: why };
}

type Status = { kind: "ran"; outcome: RunOutcome } | { kind: "undone" } | null;

export function RunPanel({ rules, current }: { rules: readonly Rule[]; current: RuleId | null }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const ui = useUiStoreApi();
  const picked = useRulesUi((s) => s.picked);
  const accountId = useUi((s) => s.activeAccountId);
  const folderId = useUi((s) => s.activeFolderId);
  // The selection's parts, so the count follows the list.
  useUi((s) => s.selectedMessageId);
  useUi((s) => s.selectedThreadId);
  useListSelection((s) => s.picked);
  const folder = useFolders(accountId).data?.find((f) => f.id === folderId);
  const [kind, setKind] = useState<RunTargetKind>("scope");
  const { busy, problem, run: act } = useBusyAction();
  const [status, setStatus] = useState<Status>(null);

  const ids = rulesToRun(picked, current);
  const selected = currentTargets(ui).targets;
  const named = folderTitle(folder, accountId);
  const folderName = named === null ? "" : "id" in named ? t(named.id) : named.text;
  const available: Record<RunTargetKind, boolean> = {
    scope: true,
    folder: folderId !== null,
    messages: selected.length > 0,
  };

  const run = () =>
    act(async () => {
      const target = await runTargetOf(client, kind, folderId, currentTargets(ui).targets);
      if (target === null) return;
      setStatus({ kind: "ran", outcome: outcomeOf(await runRules(client, ids, target)) });
      await refreshRuns(queryClient);
    });

  const undo = (outcome: RunOutcome) =>
    act(async () => {
      await undoRuns(client, outcome.undoable);
      setStatus({ kind: "undone" });
      await refreshRuns(queryClient);
    });

  const which = whichText(ids, picked, rules);
  const outcome = status?.kind === "ran" ? status.outcome : null;
  const statusText =
    status === null
      ? ""
      : status.kind === "undone"
        ? t("rules-undone-all")
        : (() => {
            const m = outcomeMessage(status.outcome);
            return t(m.id, m.args);
          })();

  return (
    <section data-part="rules.run-panel" aria-label={t("rules-run-title")}>
      <RadioGroup<RunTargetKind>
        legend={t("rules-target-legend")}
        field="run-target"
        value={kind}
        onChange={setKind}
        choices={[
          { value: "scope", label: t("rules-target-scope") },
          choice(
            "folder",
            t("rules-target-folder", { folder: folderName }),
            available.folder ? null : t("rules-target-no-folder"),
          ),
          choice(
            "messages",
            t("rules-target-messages", { count: selected.length }),
            available.messages ? null : t("rules-target-no-messages"),
          ),
        ]}
      />
      <p data-part="rules.hint">{t(which.id, which.args)}</p>
      <div data-part="rules.buttons">
        <button
          type="button"
          data-part="form.button"
          data-action="run-rules"
          disabled={busy || ids.length === 0 || !available[kind]}
          onClick={() => void run()}
        >
          {t("rules-run")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-action="undo-runs"
          disabled={busy || outcome === null || outcome.undoable.length === 0}
          onClick={() => {
            if (outcome) void undo(outcome);
          }}
        >
          {t("rules-undo-all")}
        </button>
        <Issues kind="error" lines={problem === null ? [] : [t(problem.id, problem.args)]} />
      </div>
      <p data-part="rules.run.outcome" role="status">
        {statusText}
      </p>
    </section>
  );
}
