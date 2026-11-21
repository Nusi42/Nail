/**
 * Running rules on demand (R1, ADR 0219): which messages a run acts on,
 * what it did, and undoing everything one click ran.
 *
 * - A run acts on the selected mail, on a folder, or on each rule's own
 *   messages (its folders, else the Inboxes) — `RuleRunTarget` of `./api`.
 * - The selected mail comes as mail-action targets (`../../actions/mailTargets`):
 *   a conversation row stands for its messages in the listed folder.
 * - `RunRules` answers with one run per rule; one click is undone by
 *   undoing each of them.
 */
import { expandTargets } from "../../actions/mailTargets";
import type { NailClient } from "../../api/client";
import type { FolderId, MessageTarget, RuleRun } from "../../api/generated";
import { RULE_SCOPE_TARGET, undoRuleRun, type RuleRunTarget } from "./api";

/** Where the rules window runs rules, in the order "Run on" lists them: the selected mail, a folder, each rule's own messages. */
export const RUN_TARGET_KINDS = ["messages", "folder", "scope"] as const;
export type RunTargetKind = (typeof RUN_TARGET_KINDS)[number];

/** What a run acts on, as the rules window and the menus name it. */
export type RunTarget =
  | { kind: "scope" }
  | { kind: "folder"; folderId: FolderId }
  | { kind: "messages"; targets: readonly MessageTarget[] };

/** The core's run target of `target`; `null` when it has no messages to act on. */
export async function ruleRunTargetOf(
  client: NailClient,
  target: RunTarget,
): Promise<RuleRunTarget | null> {
  switch (target.kind) {
    case "scope":
      return RULE_SCOPE_TARGET;
    case "folder":
      return { messageIds: [], folderId: target.folderId };
    case "messages": {
      const messageIds = (await expandTargets(client, target.targets)).map((m) => m.id);
      return messageIds.length === 0 ? null : { messageIds, folderId: null };
    }
  }
}

/**
 * Whether undoing a run may take something back: the core says so, or the
 * run is still finishing and queued changes (its answer can come before
 * the run is recorded as done).
 */
export function mayUndo(run: RuleRun): boolean {
  return run.undoable || (run.state === "running" && run.op_ids.length > 0);
}

/** What one click ran: how many rules, messages matched and changes queued. */
export interface RunOutcome {
  rules: number;
  matched: number;
  changes: number;
  /** The runs whose undo may take something back. */
  undoable: readonly RuleRun[];
}

export function outcomeOf(runs: readonly RuleRun[]): RunOutcome {
  return {
    rules: runs.length,
    matched: runs.reduce((n, r) => n + r.matched, 0),
    changes: runs.reduce((n, r) => n + r.op_ids.length, 0),
    undoable: runs.filter(mayUndo),
  };
}

/** The outcome's text (Fluent id and arguments). */
export function outcomeMessage(outcome: RunOutcome): {
  id: string;
  args: Record<string, number>;
} {
  return {
    id: "rules-ran",
    args: { rules: outcome.rules, matched: outcome.matched, changes: outcome.changes },
  };
}

/** Undoes every run of one click (the last one first). */
export async function undoRuns(client: NailClient, runs: readonly RuleRun[]): Promise<void> {
  for (const run of [...runs].reverse()) {
    if (mayUndo(run)) await undoRuleRun(client, run.id);
  }
}
