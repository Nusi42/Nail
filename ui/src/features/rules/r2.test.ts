/**
 * R2 (ADR 0222, rules window design review): the pure pieces — problems
 * with their fields, destructive rules, the list filter and
 * drag order, starter rules, the run-on choices, how actions and runs are
 * named, the text view's tokens and the unsaved-changes store.
 */
import type { Rule, RuleRun } from "../../api/generated";
import { createEnv } from "../../test/harness";
import { actionText, folderRefText, runTargetText, whyNoUndo, type Localize } from "./describe";
import { kdlTokens, lineNumbers } from "./kdlTokens";
import {
  actionInvalid,
  conditionInvalid,
  filterRules,
  isDestructive,
  moveItemTo,
  newRule,
  ruleIssues,
  ruleProblems,
} from "./model";
import { reorderStep } from "./RuleManager";
import { defaultRunOn, runOnChoices, type ListFacts } from "./runOn";
import { STARTER_IDS, starterRule } from "./starters";
import { rulesStore } from "./store";

/** A localizer that shows the id and its arguments. */
const t: Localize = (id, args) => (args ? `${id}${JSON.stringify(args)}` : id);

describe("rule problems with their fields", () => {
  it("names the field of each problem", () => {
    const rule: Rule = {
      ...newRule(),
      conditions: {
        type: "all",
        conditions: [
          { type: "subject", matches: { op: "contains", value: "x", case_sensitive: false } },
          { type: "any", conditions: [{ type: "tag", tag: "" }] },
        ],
      },
      actions: [{ type: "archive" }, { type: "tag", tag: " " }],
      triggers: [{ type: "schedule", interval_minutes: 0 }],
    };
    const issues = ruleIssues(rule);
    expect(issues.map((i) => i.at)).toEqual([
      { kind: "name" },
      { kind: "condition", path: [1, 0] },
      { kind: "action", index: 1 },
      { kind: "trigger", trigger: "schedule" },
    ]);
    expect(conditionInvalid(issues, [1, 0])).toBe(true);
    expect(conditionInvalid(issues, [0])).toBe(false);
    expect(actionInvalid(issues, 1)).toBe(true);
    expect(actionInvalid(issues, 0)).toBe(false);
    // The Fluent ids, each once (what the footer's badge lists).
    expect(ruleProblems(rule)).toEqual([
      "rules-error-name",
      "rules-error-condition",
      "rules-error-action",
      "rules-error-trigger",
    ]);
    expect(ruleIssues({ ...newRule(), name: "A" })).toEqual([
      { id: "rules-error-no-actions", at: { kind: "action", index: -1 } },
    ]);
  });
});

describe("the rule list helpers", () => {
  it("knows rules that take mail away", () => {
    const rule = (type: "move" | "archive" | "delete" | "flag"): Rule => ({
      ...newRule(),
      actions: [
        type === "move"
          ? { type, folder: { type: "path", path: "X", account_id: null } }
          : type === "delete"
            ? { type, permanent: false }
            : type === "flag"
              ? { type, flagged: true }
              : { type },
      ],
    });
    expect(isDestructive(rule("move"))).toBe(true);
    expect(isDestructive(rule("archive"))).toBe(true);
    expect(isDestructive(rule("delete"))).toBe(true);
    expect(isDestructive(rule("flag"))).toBe(false);
  });

  it("filters by name and moves a dragged rule", () => {
    const rules = [{ name: "Invoices" }, { name: "Newsletters" }, { name: "invites" }];
    expect(filterRules(rules, " inv ").map((r) => r.name)).toEqual(["Invoices", "invites"]);
    expect(filterRules(rules, "")).toEqual(rules);
    expect(moveItemTo(["a", "b", "c"], 2, 0)).toEqual(["c", "a", "b"]);
    expect(moveItemTo(["a", "b", "c"], 0, 2)).toEqual(["b", "c", "a"]);
    expect(moveItemTo(["a", "b"], 0, 5)).toEqual(["a", "b"]);
  });

  it("reorders with Alt+arrows (plain arrows move the focus)", () => {
    expect(reorderStep({ key: "ArrowUp", altKey: true })).toBe(-1);
    expect(reorderStep({ key: "ArrowDown", altKey: true })).toBe(1);
    expect(reorderStep({ key: "ArrowDown", altKey: false })).toBe(0);
    expect(reorderStep({ key: "Enter", altKey: true })).toBe(0);
  });

  it("starts each starter rule with its localized name", () => {
    for (const id of STARTER_IDS) {
      const rule = starterRule(id, t);
      expect(rule.id).toBe("");
      expect(rule.name).toBe(`rules-starter-${id}-name`);
      expect(rule.actions.length).toBeGreaterThan(0);
    }
    expect(starterRule("newsletters", t).actions[0]).toMatchObject({
      type: "move",
      folder: { path: "rules-starter-newsletters-folder" },
    });
    expect(starterRule("old-read", t).triggers).toEqual([{ type: "older-than", days: 30 }]);
  });
});

describe("Run on", () => {
  const facts = (over: Partial<ListFacts> = {}): ListFacts => ({
    selected: 0,
    folderName: "Inbox",
    searching: false,
    fromMessage: false,
    ...over,
  });

  it("lists the selection, the folder and the rule's own mail, with why one is unavailable", () => {
    expect(runOnChoices(facts(), t)).toEqual([
      {
        kind: "messages",
        label: 'rules-target-messages{"count":0}',
        why: "rules-target-no-messages",
      },
      { kind: "folder", label: 'rules-target-folder{"folder":"Inbox"}', why: null },
      { kind: "scope", label: "rules-target-scope", why: null },
    ]);
    const searching = runOnChoices(facts({ searching: true, selected: 2 }), t);
    expect(searching.map((c) => c.why)).toEqual([null, "rules-target-searching", null]);
    expect(runOnChoices(facts({ folderName: null }), t)[1]?.why).toBe("rules-target-no-folder");
    // R8: "select all" runs from the list's menu, not here.
    expect(runOnChoices(facts({ selected: 1, wholeList: true }), t)[0]?.why).toBe(
      "rules-target-whole-list",
    );
  });

  it("starts on the current folder when opened from a message", () => {
    expect(defaultRunOn(facts())).toBe("scope");
    expect(defaultRunOn(facts({ fromMessage: true }))).toBe("folder");
    expect(defaultRunOn(facts({ fromMessage: true, searching: true }))).toBe("scope");
  });
});

describe("how the lower panel names things", () => {
  it("names actions with their values", () => {
    expect(actionText(t, { type: "tag", tag: "project" })).toBe(
      'rules-do-value{"action":"rules-do-tag","value":"project"}',
    );
    expect(actionText(t, { type: "archive" })).toBe("rules-do-archive");
    expect(actionText(t, { type: "flag", flagged: false })).toBe("rules-set-flag-off");
    expect(actionText(t, { type: "delete", permanent: false })).toBe("rules-do-delete");
    expect(folderRefText(t, { type: "role", role: "archive", account_id: null })).toBe(
      "rules-role-archive",
    );
  });

  it("names what a run acted on", () => {
    const name = (id: string) => (id === "f1" ? "Lists" : null);
    expect(runTargetText(t, { kind: "scope" }, name)).toBe("rules-run-target-scope");
    expect(runTargetText(t, { kind: "messages", count: 3 }, name)).toBe(
      'rules-run-target-messages{"count":3}',
    );
    expect(runTargetText(t, { kind: "folder", folder_id: "f1" }, name)).toBe(
      'rules-run-target-folder{"folder":"Lists"}',
    );
    expect(runTargetText(t, { kind: "folder", folder_id: "f2" }, name)).toContain('"folder":"f2"');
  });

  it("says why a run cannot be undone", () => {
    const run = (over: Partial<RuleRun>): RuleRun => ({
      id: "r",
      rule_id: "x",
      rule_name: "X",
      cause: "manual",
      state: "done",
      started_at: "",
      finished_at: null,
      target: { kind: "scope" },
      matched: 1,
      op_ids: ["o"],
      undoable: false,
      message: null,
      ...over,
    });
    expect(whyNoUndo(run({ undoable: true }))).toBeNull();
    expect(whyNoUndo(run({ state: "undone" }))).toBe("rules-run-why-undone");
    expect(whyNoUndo(run({ state: "running" }))).toBe("rules-run-why-running");
    expect(whyNoUndo(run({ state: "failed" }))).toBe("rules-run-why-failed");
    expect(whyNoUndo(run({ op_ids: [] }))).toBe("rules-run-why-nothing");
    expect(whyNoUndo(run({}))).toBe("rules-run-why-final");
  });
});

describe("the text view's tokens", () => {
  const text = `// Lists\nrule "News" trigger="arrival" enabled=#false {\n    when { size greater=100 }\n    then { tag "news"; }\n}\n`;

  it("gives the text back and colours each kind", () => {
    const tokens = kdlTokens(text);
    expect(tokens.map((x) => x.text).join("")).toBe(text);
    const of = (kind: string) => tokens.filter((x) => x.kind === kind).map((x) => x.text);
    expect(of("comment")).toEqual(["// Lists"]);
    expect(of("node")).toEqual(["rule", "when", "size", "then", "tag"]);
    expect(of("property")).toEqual(["trigger", "enabled", "greater"]);
    expect(of("string")).toEqual(['"News"', '"arrival"', '"news"']);
    expect(of("keyword")).toEqual(["#false"]);
    expect(of("number")).toEqual(["100"]);
  });

  it("never loses text it does not understand", () => {
    for (const odd of ['rule "open', "/* never closed", "a\\b", "#", "rule {{ }}}", ""]) {
      expect(
        kdlTokens(odd)
          .map((x) => x.text)
          .join(""),
      ).toBe(odd);
    }
    expect(lineNumbers("a\nb\n")).toEqual([1, 2, 3]);
  });
});

describe("unsaved changes", () => {
  it("asks before another rule opens, and opens it after the prompt's answer", () => {
    const env = createEnv();
    const store = rulesStore(env.client);
    /** The prompt's pending answers: calling one is Save or Don't save, dropping it Cancel. */
    const asked: (() => void)[] = [];
    store.getState().setLeave((proceed) => {
      asked.push(proceed);
    });
    store.getState().select("a");
    // Nothing unsaved: no question.
    expect(asked).toHaveLength(0);
    expect(store.getState().editing).toMatchObject({ id: "a" });
    store.getState().setDirty(true);
    store.getState().select("b");
    expect(asked).toHaveLength(1);
    // Cancel: the rule stays.
    asked.length = 0;
    expect(store.getState().editing).toMatchObject({ id: "a" });
    // The rule open already is no question.
    store.getState().select("a");
    expect(asked).toHaveLength(0);
    store.getState().startNew(newRule());
    asked.shift()?.();
    expect(store.getState().editing).toMatchObject({ kind: "new" });
    expect(store.getState().dirty).toBe(false);
  });

  it("opens another rule at once without an editor to ask", () => {
    const env = createEnv();
    const store = rulesStore(env.client);
    store.getState().select("a");
    store.getState().setDirty(true);
    store.getState().select("b");
    expect(store.getState().editing).toMatchObject({ id: "b" });
  });
});
