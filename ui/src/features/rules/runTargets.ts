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
import type { NailClient } from "../../api/client";
import type { FolderId, MessageId, MessageTarget, RuleRun } from "../../api/generated";
import { RULE_SCOPE_TARGET, undoRuleRun, type RuleRunTarget } from "./api";

/** Where the rules window runs rules: each rule's own messages, a folder, the selected mail. */
export const RUN_TARGET_KINDS = ["scope", "folder", "messages"] as const;
export type RunTargetKind = (typeof RUN_TARGET_KINDS)[number];

/** The message ids of mail-action targets, each once, in order. */
export async function messageIdsOf(
  client: NailClient,
  targets: readonly MessageTarget[],
): Promise<MessageId[]> {
  const ids: MessageId[] = [];
  for (const target of targets) {
    if (target.kind === "message") {
      ids.push(target.message_id);
    } else {
      const conversation = await client.request(
        { type: "GetConversation", thread_id: target.thread_id, folder_id: target.folder_id },
        "Conversation",
      );
      ids.push(...conversation.messages.map((m) => m.id));
    }
  }
  return [...new Set(ids)];
}

/** The run target of a kind; `null` when it has nothing to act on. */
export async function runTargetOf(
  client: NailClient,
  kind: RunTargetKind,
  folderId: FolderId | null,
  targets: readonly MessageTarget[],
): Promise<RuleRunTarget | null> {
  switch (kind) {
    case "scope":
      return RULE_SCOPE_TARGET;
    case "folder":
      return folderId === null ? null : { messageIds: [], folderId };
    case "messages": {
      const messageIds = await messageIdsOf(client, targets);
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
