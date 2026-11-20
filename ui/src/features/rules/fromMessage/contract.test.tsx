/**
 * R2 contract (ADR 0222): the rules core requests the redesigned window
 * and "values from this mail" build on, answered by the mock core like the
 * core: message facts, drafts from the text view, run targets, and a run
 * history that goes with its rule.
 */
import { createEnv } from "../../../test/harness";
import { must } from "../../../test/must";
import { deleteRule, ruleFromText, runRules, saveRule } from "../api";
import { newRule } from "../model";
import { rulesStore } from "../store";
import { getMessageFacts, valuesForCondition } from "./facts";

describe("rules R2 contract (mock core)", () => {
  it("answers the facts of a message, and not_found for an unknown one", async () => {
    const env = createEnv();
    const message = must(env.transport.data.messages[0]);
    const facts = await getMessageFacts(env.client, message.id);
    expect(facts.message_id).toBe(message.id);
    expect(facts.folder_id).toBe(message.folder_id);
    expect(facts.subject).toBe(message.subject);
    expect(facts.header_names).toContain("Subject");
    const subject = { op: "contains", value: "", case_sensitive: false } as const;
    expect(valuesForCondition(facts, { type: "subject", matches: subject })).toEqual([]);
    await expect(getMessageFacts(env.client, "missing")).rejects.toThrow();
  });

  it("keeps a draft without a name and reports the gap", async () => {
    const env = createEnv();
    const text = ['rule "" {', "}", ""].join("\n");
    const strict = await ruleFromText(env.client, text);
    expect(strict.rule).toBeNull();
    expect(strict.errors).toHaveLength(1);
    const draft = await ruleFromText(env.client, text, true);
    expect(draft.rule?.name).toBe("");
    expect(draft.errors).toHaveLength(1);
  });

  it("records each run's target and deletes the runs with their rule", async () => {
    const env = createEnv();
    const message = must(env.transport.data.messages[0]);
    const rule = await saveRule(env.client, { ...newRule(), name: "Mark all" });
    const [onMessages] = await runRules(env.client, [rule.id], {
      messageIds: [message.id],
      folderId: null,
    });
    expect(onMessages?.target).toEqual({ kind: "messages", count: 1 });
    const [onFolder] = await runRules(env.client, [rule.id], {
      messageIds: [],
      folderId: message.folder_id,
    });
    expect(onFolder?.target).toEqual({ kind: "folder", folder_id: message.folder_id });
    const [onScope] = await runRules(env.client, [rule.id]);
    expect(onScope?.target).toEqual({ kind: "scope" });
    const runs = async () =>
      (await env.client.request({ type: "ListRuleRuns", rule_id: rule.id, limit: 10 }, "RuleRuns"))
        .runs;
    expect(await runs()).toHaveLength(3);
    await deleteRule(env.client, rule.id);
    expect(await runs()).toEqual([]);
  });

  it("remembers the message the window was opened from", () => {
    const env = createEnv();
    const store = rulesStore(env.client);
    expect(store.getState().origin).toBeNull();
    store.getState().setOrigin({ messageId: "m1", folderId: "f1" });
    expect(store.getState().origin).toEqual({ messageId: "m1", folderId: "f1" });
  });
});
