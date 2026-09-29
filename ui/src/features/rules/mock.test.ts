import type { Event as NailEvent, Rule, RuleRun } from "../../api/generated";
import { createEnv } from "../../test/harness";
import { newRule } from "./model";
import type { ExplainedMatch } from "./types";

function setup() {
  const env = createEnv();
  const events: NailEvent[] = [];
  env.transport.onEvent((e) => events.push(e));
  return { env, client: env.client, events, data: env.transport.data };
}

const rustRule = (): Rule => ({
  ...newRule(),
  name: "Rust list",
  conditions: {
    type: "any",
    conditions: [
      { type: "subject", matches: { op: "starts-with", value: "[rust", case_sensitive: false } },
    ],
  },
  actions: [{ type: "tag", tag: "rust" }, { type: "archive" }],
});

describe("rules mock", () => {
  it("lists, saves, reorders and deletes rules", async () => {
    const { client, events } = setup();
    const list = async () => (await client.request({ type: "ListRules" }, "Rules")).rules;
    expect((await list()).map((r) => r.id)).toEqual(["rust-announcements"]);

    const saved = (
      await client.request({ type: "SaveRule", rule: rustRule(), position: 0 }, "RuleSaved")
    ).rule;
    expect(saved.id).toBe("rust-list");
    expect(saved.source).toEqual({ file: "rules.kdl", line: 1, column: 1 });
    const again = (
      await client.request({ type: "SaveRule", rule: rustRule(), position: null }, "RuleSaved")
    ).rule;
    expect(again.id).toBe("rust-list-2");
    expect((await list()).map((r) => r.id)).toEqual([
      "rust-list",
      "rust-announcements",
      "rust-list-2",
    ]);
    await Promise.resolve();
    expect(events.some((e) => e.type === "RulesChanged")).toBe(true);

    // Replacing keeps the place.
    await client.request(
      { type: "SaveRule", rule: { ...saved, enabled: false }, position: 2 },
      "RuleSaved",
    );
    expect((await list())[0]).toMatchObject({ id: "rust-list", enabled: false });

    await client.request(
      { type: "ReorderRules", rule_ids: ["rust-list-2", "rust-list", "rust-announcements"] },
      "Done",
    );
    expect((await list()).map((r) => r.id)).toEqual([
      "rust-list-2",
      "rust-list",
      "rust-announcements",
    ]);
    await expect(
      client.request({ type: "ReorderRules", rule_ids: ["rust-list"] }, "Done"),
    ).rejects.toMatchObject({ code: "invalid_request" });

    await client.request({ type: "DeleteRule", rule_id: "rust-list-2" }, "Done");
    expect((await list()).map((r) => r.id)).toEqual(["rust-list", "rust-announcements"]);
    await expect(
      client.request({ type: "DeleteRule", rule_id: "nope" }, "Done"),
    ).rejects.toMatchObject({ code: "not_found" });
  });

  it("refuses rules without a name or with a broken regular expression", async () => {
    const { client } = setup();
    await expect(
      client.request({ type: "SaveRule", rule: newRule(), position: null }, "RuleSaved"),
    ).rejects.toMatchObject({ code: "invalid_request" });
    const broken: Rule = {
      ...rustRule(),
      conditions: {
        type: "all",
        conditions: [
          { type: "subject", matches: { op: "regex", value: "(", case_sensitive: false } },
        ],
      },
    };
    await expect(
      client.request({ type: "SaveRule", rule: broken, position: null }, "RuleSaved"),
    ).rejects.toMatchObject({ code: "invalid_request" });
    await expect(
      client.request({ type: "TestRule", rule: broken, folder_id: null, limit: 5 }, "RuleTested"),
    ).rejects.toMatchObject({ code: "invalid_request" });
  });

  it("tests a rule without changing anything and explains the matches", async () => {
    const { client, data } = setup();
    const before = JSON.stringify(data.messages);
    const r = await client.request(
      { type: "TestRule", rule: rustRule(), folder_id: null, limit: 10 },
      "RuleTested",
    );
    expect(r.scanned).toBe(data.messages.length);
    expect(r.matches.map((m) => m.message.subject)).toEqual([
      "[rust-announce] Rust 1.97 released",
      "[rust-announce] Rust 1.97 released",
    ]);
    const match = r.matches[0] as ExplainedMatch;
    expect(match.actions).toEqual(rustRule().actions);
    expect(match.explanation).toEqual([
      { path: [], matched: true, value: null },
      { path: [0], matched: true, value: "[rust-announce] Rust 1.97 released" },
    ]);
    expect(JSON.stringify(data.messages)).toBe(before);

    const limited = await client.request(
      { type: "TestRule", rule: rustRule(), folder_id: null, limit: 1 },
      "RuleTested",
    );
    expect(limited.matches).toHaveLength(1);
    const inFolder = await client.request(
      { type: "TestRule", rule: rustRule(), folder_id: data.folders[0]?.id ?? "", limit: 10 },
      "RuleTested",
    );
    expect(inFolder.matches).toEqual([]);
  });

  it("runs a rule through the queue and undoes the run as one unit", async () => {
    const { client, data, events } = setup();
    data.behaviour.workMs = 0;
    const saved = (
      await client.request({ type: "SaveRule", rule: rustRule(), position: null }, "RuleSaved")
    ).rule;
    const listPosts = () => data.messages.filter((m) => m.subject.startsWith("[rust"));
    const folders = listPosts().map((m) => m.folder_id);

    const ran = (
      await client.request(
        { type: "RunRule", rule_id: saved.id, message_ids: [], folder_id: null },
        "RuleRan",
      )
    ).run;
    expect(ran).toMatchObject({ state: "running", matched: 2 });
    // tag + archive for each (already in Archive: the move is skipped).
    expect(ran.op_ids).toHaveLength(2);
    expect(listPosts().every((m) => m.tags.includes("rust"))).toBe(true);

    await vi.waitFor(() =>
      expect(events.some((e) => e.type === "RuleRunFinished" && e.run.state === "done")).toBe(true),
    );
    const runs = (
      await client.request({ type: "ListRuleRuns", rule_id: saved.id, limit: 5 }, "RuleRuns")
    ).runs;
    expect(runs[0]).toMatchObject({ id: ran.id, state: "done", undoable: true });

    await client.request({ type: "UndoRuleRun", run_id: ran.id }, "Queued");
    expect(listPosts().every((m) => !m.tags.includes("rust"))).toBe(true);
    expect(listPosts().map((m) => m.folder_id)).toEqual(folders);
    const after = (
      await client.request({ type: "ListRuleRuns", rule_id: null, limit: 5 }, "RuleRuns")
    ).runs[0] as RuleRun;
    expect(after).toMatchObject({ state: "undone", undoable: false });
    await expect(
      client.request({ type: "UndoRuleRun", run_id: ran.id }, "Queued"),
    ).rejects.toMatchObject({ code: "conflict" });
  });

  it("moves messages and moves them back on undo", async () => {
    const { client, data, events } = setup();
    data.behaviour.workMs = 0;
    const inbox = data.folders.find((f) => f.account_id === "acc-work" && f.role === "inbox");
    const target = data.messages.find((m) => m.folder_id === inbox?.id) as { id: string };
    const rule: Rule = {
      ...rustRule(),
      name: "File it",
      conditions: { type: "all", conditions: [] },
      actions: [
        { type: "move", folder: { type: "path", path: "Archive", account_id: null } },
        { type: "stop" },
        { type: "flag", flagged: true },
      ],
    };
    const saved = (await client.request({ type: "SaveRule", rule, position: null }, "RuleSaved"))
      .rule;
    const flaggedBefore = data.messages.find((m) => m.id === target.id)?.flags.flagged;
    const ran = (
      await client.request(
        { type: "RunRule", rule_id: saved.id, message_ids: [target.id], folder_id: null },
        "RuleRan",
      )
    ).run;
    const moved = data.messages.find((m) => m.id === target.id);
    expect(data.folders.find((f) => f.id === moved?.folder_id)?.role).toBe("archive");
    // Nothing after `stop` ran.
    expect(moved?.flags.flagged).toBe(flaggedBefore);
    await vi.waitFor(() => expect(events.some((e) => e.type === "RuleRunFinished")).toBe(true));
    await client.request({ type: "UndoRuleRun", run_id: ran.id }, "Queued");
    expect(data.messages.find((m) => m.id === target.id)?.folder_id).toBe(inbox?.id);
  });

  it("converts rules to and from text", async () => {
    const { client } = setup();
    const text = (await client.request({ type: "RuleToText", rule: rustRule() }, "RuleText")).text;
    expect(text).toContain('rule "Rust list" trigger="arrival"');
    const parsed = await client.request({ type: "RuleFromText", text }, "RuleParsed");
    expect(parsed.errors).toEqual([]);
    expect(parsed.rule?.actions).toEqual(rustRule().actions);
    const bad = await client.request({ type: "RuleFromText", text: "nonsense {" }, "RuleParsed");
    expect(bad.rule).toBeNull();
    expect(bad.errors.length).toBe(1);
  });
});
