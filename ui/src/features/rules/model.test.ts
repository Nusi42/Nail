import type { Condition, MessageSummary, Rule } from "../../api/generated";
import {
  ACTION_KINDS,
  CONDITION_KINDS,
  addChild,
  comparePaths,
  conditionAt,
  moveItem,
  newAction,
  newCondition,
  newRule,
  parseAddresses,
  removeAt,
  replaceAt,
  ruleFromMessage,
  ruleProblems,
  setGroupMode,
  toggleTrigger,
} from "./model";

const tag = (t: string): Condition => ({ type: "tag", tag: t });

const tree: Condition = {
  type: "all",
  conditions: [
    tag("a"),
    { type: "any", conditions: [tag("b"), tag("c")] },
    { type: "not", condition: tag("d") },
  ],
};

describe("rules model", () => {
  it("creates a blank condition and action of every kind", () => {
    for (const kind of CONDITION_KINDS) expect(newCondition(kind).type).toBe(kind);
    for (const kind of ACTION_KINDS) expect(newAction(kind).type).toBe(kind);
    expect(newRule()).toMatchObject({ id: "", enabled: true, triggers: [{ type: "arrival" }] });
  });

  it("finds, replaces, adds and removes conditions by path", () => {
    expect(conditionAt(tree, [])).toBe(tree);
    expect(conditionAt(tree, [1, 1])).toEqual(tag("c"));
    expect(conditionAt(tree, [2, 0])).toEqual(tag("d"));
    expect(conditionAt(tree, [5])).toBeUndefined();

    const replaced = replaceAt(tree, [1, 0], tag("x"));
    expect(conditionAt(replaced, [1, 0])).toEqual(tag("x"));
    expect(conditionAt(tree, [1, 0])).toEqual(tag("b")); // unchanged input

    const added = addChild(tree, [1], tag("e"));
    expect(conditionAt(added, [1, 2])).toEqual(tag("e"));
    // A `not` gets its child replaced; leaves take no children.
    expect(conditionAt(addChild(tree, [2], tag("f")), [2, 0])).toEqual(tag("f"));
    expect(addChild(tree, [0], tag("g"))).toBe(tree);

    const removed = removeAt(tree, [1, 0]);
    expect(conditionAt(removed, [1])).toEqual({ type: "any", conditions: [tag("c")] });
    // Removing the only child of a `not` removes the `not`.
    expect(removeAt(tree, [2, 0])).toEqual({
      type: "all",
      conditions: [tag("a"), { type: "any", conditions: [tag("b"), tag("c")] }],
    });
    expect(removeAt(tree, [])).toBe(tree);

    expect(setGroupMode(tree, [1], "all")).toMatchObject({
      conditions: [tag("a"), { type: "all" }, { type: "not" }],
    });
    expect(setGroupMode(tree, [0], "any")).toBe(tree);
  });

  it("orders paths as a tree and moves items", () => {
    const paths = [[1, 10], [1, 2], [], [0], [1]];
    expect(paths.sort(comparePaths)).toEqual([[], [0], [1], [1, 2], [1, 10]]);
    expect(moveItem(["a", "b", "c"], 0, 1)).toEqual(["b", "a", "c"]);
    expect(moveItem(["a", "b", "c"], 2, 1)).toEqual(["a", "b", "c"]);
    expect(moveItem(["a", "b", "c"], 0, -1)).toEqual(["a", "b", "c"]);
  });

  it("lists what keeps a rule from being saved", () => {
    const rule = newRule();
    expect(ruleProblems(rule)).toEqual(["rules-error-name", "rules-error-no-actions"]);
    const filled: Rule = {
      ...rule,
      name: "x",
      conditions: { type: "all", conditions: [newCondition("subject"), newCondition("date")] },
      actions: [newAction("move"), newAction("forward")],
      triggers: [{ type: "schedule", interval_minutes: 0 }],
    };
    expect(ruleProblems(filled).sort()).toEqual([
      "rules-error-action",
      "rules-error-condition",
      "rules-error-trigger",
    ]);
    const ok: Rule = {
      ...filled,
      conditions: { type: "all", conditions: [{ type: "query", query: "is:unread" }] },
      actions: [{ type: "forward", to: ["a@b.example"], identity_id: null }],
      triggers: [{ type: "arrival" }],
    };
    expect(ruleProblems(ok)).toEqual([]);
  });

  it("toggles triggers with default parameters", () => {
    const rule = newRule();
    const scheduled = toggleTrigger(rule, "schedule", true);
    expect(scheduled.triggers).toEqual([
      { type: "arrival" },
      { type: "schedule", interval_minutes: 60 },
    ]);
    expect(toggleTrigger(scheduled, "arrival", false).triggers).toEqual([
      { type: "schedule", interval_minutes: 60 },
    ]);
    expect(toggleTrigger(rule, "older-than", true).triggers[1]).toEqual({
      type: "older-than",
      days: 30,
    });
  });

  it("parses address lists", () => {
    expect(parseAddresses(" a@x.example, b@y.example;c@z.example  ")).toEqual([
      "a@x.example",
      "b@y.example",
      "c@z.example",
    ]);
    expect(parseAddresses("")).toEqual([]);
  });

  it("pre-fills a rule from a message", () => {
    const message = {
      id: "m1",
      subject: "[rust-announce] Rust 1.97 released",
      from: { name: "Rust Announce", email: "announce@lists.example" },
    } as MessageSummary;
    const rule = ruleFromMessage(message);
    expect(rule.id).toBe("");
    expect(rule.name).toBe("Rust Announce");
    expect(rule.conditions).toEqual({
      type: "all",
      conditions: [
        {
          type: "address",
          field: "from",
          matches: { op: "equals", value: "announce@lists.example", case_sensitive: false },
        },
        {
          type: "subject",
          matches: { op: "starts-with", value: "[rust-announce]", case_sensitive: false },
        },
      ],
    });
    const anonymous = ruleFromMessage({ ...message, from: null, subject: "Hello" });
    expect(anonymous.name).toBe("Hello");
    expect(anonymous.conditions).toEqual({
      type: "all",
      conditions: [
        { type: "subject", matches: { op: "equals", value: "Hello", case_sensitive: false } },
      ],
    });
  });
});
