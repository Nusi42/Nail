/**
 * Rules data through the `NailClient`: queries (rules, runs) and the
 * requests the editor sends. The core owns `rules.kdl`; every change is a
 * request and comes back as `RulesChanged` / `RuleRunFinished`.
 */
import { useQuery, type QueryClient } from "@tanstack/react-query";
import type { NailClient } from "../../api/client";
import type {
  Event as NailEvent,
  FolderId,
  MessageId,
  Rule,
  RuleId,
  RuleRun,
} from "../../api/generated";
import { useClient } from "../../state/queries";
import type { ExplainedMatch } from "./types";

export const rulesKeys = {
  rules: ["rules"] as const,
  runs: (ruleId: RuleId | null) => ["rule-runs", ruleId] as const,
  allRuns: ["rule-runs"] as const,
};

/** The saved rules, in file order. */
export async function listRules(client: NailClient): Promise<Rule[]> {
  return (await client.request({ type: "ListRules" }, "Rules")).rules;
}

/** The query of the saved rules (shared by the window and the menus). */
export function rulesQuery(client: NailClient) {
  return { queryKey: rulesKeys.rules, queryFn: () => listRules(client) };
}

export function useRules() {
  return useQuery(rulesQuery(useClient()));
}

export function useRuleRuns(ruleId: RuleId | null, limit = 10) {
  const client = useClient();
  return useQuery({
    queryKey: rulesKeys.runs(ruleId),
    queryFn: async () =>
      (await client.request({ type: "ListRuleRuns", rule_id: ruleId, limit }, "RuleRuns")).runs,
    enabled: ruleId !== null,
  });
}

export async function saveRule(
  client: NailClient,
  rule: Rule,
  position: number | null = null,
): Promise<Rule> {
  return (await client.request({ type: "SaveRule", rule, position }, "RuleSaved")).rule;
}

export async function deleteRule(client: NailClient, ruleId: RuleId): Promise<void> {
  await client.request({ type: "DeleteRule", rule_id: ruleId }, "Done");
}

export async function reorderRules(client: NailClient, ruleIds: RuleId[]): Promise<void> {
  await client.request({ type: "ReorderRules", rule_ids: ruleIds }, "Done");
}

export async function testRule(
  client: NailClient,
  rule: Rule,
  folderId: FolderId | null,
  limit: number,
): Promise<{ scanned: number; matches: ExplainedMatch[] }> {
  const r = await client.request(
    { type: "TestRule", rule, folder_id: folderId, limit },
    "RuleTested",
  );
  return { scanned: r.scanned, matches: r.matches };
}

/**
 * Where a run by hand acts: the named messages, else the folder, else each
 * rule's own messages (its folders, else the inboxes). R1, ADR 0219.
 */
export interface RuleRunTarget {
  messageIds: readonly MessageId[];
  folderId: FolderId | null;
}

/** Each rule's own messages. */
export const RULE_SCOPE_TARGET: RuleRunTarget = { messageIds: [], folderId: null };

/** Runs one saved rule now on `target`. */
export async function runRule(
  client: NailClient,
  ruleId: RuleId,
  target: RuleRunTarget = RULE_SCOPE_TARGET,
): Promise<RuleRun> {
  return (
    await client.request(
      {
        type: "RunRule",
        rule_id: ruleId,
        message_ids: [...target.messageIds],
        folder_id: target.folderId,
      },
      "RuleRan",
    )
  ).run;
}

/**
 * Runs several saved rules now on `target`, in file order, one run each
 * (each undone on its own with `undoRuleRun`).
 */
export async function runRules(
  client: NailClient,
  ruleIds: readonly RuleId[],
  target: RuleRunTarget = RULE_SCOPE_TARGET,
): Promise<RuleRun[]> {
  return (
    await client.request(
      {
        type: "RunRules",
        rule_ids: [...ruleIds],
        message_ids: [...target.messageIds],
        folder_id: target.folderId,
      },
      "RulesRan",
    )
  ).runs;
}

export async function undoRuleRun(client: NailClient, runId: string): Promise<void> {
  await client.request({ type: "UndoRuleRun", run_id: runId }, "Queued");
}

export async function ruleToText(client: NailClient, rule: Rule): Promise<string> {
  return (await client.request({ type: "RuleToText", rule }, "RuleText")).text;
}

export async function ruleFromText(client: NailClient, text: string) {
  return client.request({ type: "RuleFromText", text }, "RuleParsed");
}

/** Refetches what an event changed. */
export function onRulesEvent(event: NailEvent, queryClient: QueryClient): void {
  if (event.type === "RulesChanged") {
    void queryClient.invalidateQueries({ queryKey: rulesKeys.rules });
  } else if (event.type === "RuleRunFinished") {
    void queryClient.invalidateQueries({ queryKey: rulesKeys.allRuns });
  }
}
