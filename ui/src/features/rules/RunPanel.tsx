/**
 * Running rules from the rules window (R1, ADR 0219): the rules checked in
 * the list (else the rule being edited) on a target — each rule's own
 * messages (its folders, else the Inboxes; what "Run now" always did), the
 * folder the list shows, or the mail selected in the list (with the count;
 * unavailable, with the reason, while nothing is selected or the list shows
 * search results). A rule just written is applied at once, without leaving
 * the window.
 *
 * The panel sits under the editor's Save / Test buttons (review R1: in the
 * list column it was out of view). The core runs saved rules only, so when
 * the rule being edited is new or has unsaved changes and it is one of the
 * rules to run, the button is "Save and run": it saves the editor's rule
 * first and never runs the old saved one behind the user's back.
 *
 * The outcome line (matched messages, changes queued) is always there, so
 * it never moves anything; Undo takes back every run of the last click.
 * Errors are the compact `Issues` indicator in the button row.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { currentTargets, useListSelection } from "../../actions/mailTargets";
import type { RuleId } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useBusyAction } from "../../state/busyAction";
import { useClient, useFolders } from "../../state/queries";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { listTitle } from "../../widgets/ListHeader";
import { RadioGroup, type Choice } from "../portability/Fields";
import { runRules } from "./api";
import { Issues } from "./Issues";
import { refreshRuns } from "./runMenu";
import {
  outcomeMessage,
  outcomeOf,
  ruleRunTargetOf,
  undoRuns,
  type RunOutcome,
  type RunTarget,
  type RunTargetKind,
} from "./runTargets";
import { useRulesUi } from "./store";

/** The rule open in the editor, as the run panel needs it. */
export interface EditedRule {
  /** Its saved id; `null` while it is new. */
  id: RuleId | null;
  name: string;
  /** It differs from the saved rule (or is new). */
  unsaved: boolean;
  /** It cannot be saved yet (something is missing). */
  blocked: boolean;
  /** Saves it; its id, or `null` when it could not be saved. */
  save: () => Promise<RuleId | null>;
}

/** What one click does: save the edited rule first, then run `ids`. */
export interface RunPlan {
  /** The saved rules to run (an edited rule saved first is added once saved). */
  ids: RuleId[];
  /** The edited rule is saved before the run. */
  saveFirst: boolean;
}

/** The rules a click runs: the checked ones, else the rule being edited. */
export function runPlan(picked: readonly RuleId[], edited: EditedRule | null): RunPlan {
  const runsEdited =
    edited !== null && (picked.length === 0 || (edited.id !== null && picked.includes(edited.id)));
  const saveFirst = runsEdited && edited.unsaved;
  if (picked.length > 0) return { ids: [...picked], saveFirst };
  return { ids: edited?.id != null && !saveFirst ? [edited.id] : [], saveFirst };
}

/** What the panel says about the rules it runs (Fluent id and arguments). */
function whichText(
  plan: RunPlan,
  picked: readonly RuleId[],
  edited: EditedRule | null,
): { id: string; args?: Record<string, string | number> } {
  if (picked.length > 0) return { id: "rules-run-which-checked", args: { count: picked.length } };
  if (edited === null) return { id: "rules-run-which-none" };
  if (plan.saveFirst && edited.blocked) return { id: "rules-run-which-fix" };
  return {
    id: plan.saveFirst ? "rules-run-which-save" : "rules-run-which-current",
    args: { name: edited.name },
  };
}

/** A target choice; `why` says why it is not available (`null` = it is). */
function choice(value: RunTargetKind, label: string, why: string | null): Choice<RunTargetKind> {
  return why === null ? { value, label } : { value, label, disabled: true, hint: why };
}

type Status = { kind: "ran"; outcome: RunOutcome } | { kind: "undone" } | null;

export function RunPanel({ edited }: { edited: EditedRule | null }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const ui = useUiStoreApi();
  const picked = useRulesUi((s) => s.picked);
  const accountId = useUi((s) => s.activeAccountId);
  const folderId = useUi((s) => s.activeFolderId);
  const search = useUi((s) => s.search);
  // The selection's parts, so the count follows the list.
  useUi((s) => s.selectedMessageId);
  useUi((s) => s.selectedThreadId);
  useListSelection((s) => s.picked);
  const folder = useFolders(accountId).data?.find((f) => f.id === folderId);
  const [kind, setKind] = useState<RunTargetKind>("scope");
  const { busy, problem, run: act } = useBusyAction();
  const [status, setStatus] = useState<Status>(null);

  const plan = runPlan(picked, edited);
  const selected = currentTargets(ui).targets;
  // The folder as the list header names it; a search (a query, a smart
  // folder, a favorite) lists no folder.
  const listed = search === null ? listTitle(null, folder, accountId) : null;
  const folderName = listed === null ? "" : "id" in listed ? t(listed.id) : listed.text;
  const folderWhy =
    search !== null
      ? t("rules-target-searching")
      : folderId === null || listed === null
        ? t("rules-target-no-folder")
        : null;
  const available: Record<RunTargetKind, boolean> = {
    scope: true,
    folder: folderWhy === null,
    messages: selected.length > 0,
  };
  const ready = plan.saveFirst ? edited?.blocked === false : plan.ids.length > 0;

  /** The target of the chosen kind, read when the button is clicked. */
  const targetOf = (): RunTarget | null => {
    switch (kind) {
      case "scope":
        return { kind: "scope" };
      case "folder":
        return folderId === null ? null : { kind: "folder", folderId };
      case "messages":
        return { kind: "messages", targets: currentTargets(ui).targets };
    }
  };

  const run = () =>
    act(async () => {
      const chosen = targetOf();
      const target = chosen === null ? null : await ruleRunTargetOf(client, chosen);
      if (target === null) return;
      let ids = plan.ids;
      if (plan.saveFirst && edited) {
        const saved = await edited.save();
        if (saved === null) return;
        if (!ids.includes(saved)) ids = [...ids, saved];
      }
      setStatus({ kind: "ran", outcome: outcomeOf(await runRules(client, ids, target)) });
      await refreshRuns(queryClient);
    });

  const undo = (outcome: RunOutcome) =>
    act(async () => {
      await undoRuns(client, outcome.undoable);
      setStatus({ kind: "undone" });
      await refreshRuns(queryClient);
    });

  const which = whichText(plan, picked, edited);
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
          choice("folder", t("rules-target-folder", { folder: folderName }), folderWhy),
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
          disabled={busy || !ready || !available[kind]}
          onClick={() => void run()}
        >
          {t(plan.saveFirst ? "rules-save-and-run" : "rules-run")}
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
