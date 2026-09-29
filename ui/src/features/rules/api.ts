/**
 * Rules data through the `NailClient`: queries (rules, runs) and the
 * requests the editor sends. The core owns `rules.kdl`; every change is a
 * request and comes back as `RulesChanged` / `RuleRunFinished`.
 */
import { useQuery, type QueryClient } from "@tanstack/react-query";
import type { NailClient } from "../../api/client";
import type { Event as NailEvent, FolderId, Rule, RuleId } from "../../api/generated";
import { useClient } from "../../state/queries";
import type { ExplainedMatch } from "./types";

export const rulesKeys = {
  rules: ["rules"] as const,
  runs: (ruleId: RuleId | null) => ["rule-runs", ruleId] as const,
  allRuns: ["rule-runs"] as const,
};

export function useRules() {
  const client = useClient();
  return useQuery({
    queryKey: rulesKeys.rules,
    queryFn: async () => (await client.request({ type: "ListRules" }, "Rules")).rules,
  });
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

export async function runRule(client: NailClient, ruleId: RuleId, messageIds: string[] = []) {
  return (
    await client.request(
      { type: "RunRule", rule_id: ruleId, message_ids: messageIds, folder_id: null },
      "RuleRan",
    )
  ).run;
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
