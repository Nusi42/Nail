/**
 * The mock core's answers to the rules requests (WP22), used by
 * `MockTransport` (`../../api/mock.ts` routes exactly these requests here).
 * State lives in `data.features.rules` ([`createRulesMockState`]).
 *
 * Rules are kept in memory in file order; `TestRule` and `RunRule` use the
 * mock evaluator (`mockEval.ts`); a run's writes go through the mock's
 * queued writes (`updateFlags`, `moveMessages`, `deleteMessages`) and are
 * undone as one unit.
 */
import type {
  FlagChange,
  MessageSummary,
  Request,
  Response,
  Rule,
  RuleAction,
  RuleRun,
} from "../../api/generated";
import {
  deleteMessages,
  errorResponse,
  folderMessages,
  moveMessages,
  updateFlags,
  type MockCore,
} from "../../api/mockMail";
import { MOCK_NOW } from "../../api/mockData";
import { explain, resolveFolder } from "./mockEval";
import { ruleFromText, ruleIdBase, ruleToText } from "./mockText";
import type { ExplainedMatch } from "./types";

/** The requests this feature answers. */
export const RULES_REQUESTS = [
  "ListRules",
  "SaveRule",
  "DeleteRule",
  "ReorderRules",
  "TestRule",
  "RunRule",
  "ListRuleRuns",
  "UndoRuleRun",
  "RuleToText",
  "RuleFromText",
] as const satisfies readonly Request["type"][];

export type RulesRequest = Extract<Request, { type: (typeof RULES_REQUESTS)[number] }>;

export function isRulesRequest(request: Request): request is RulesRequest {
  return (RULES_REQUESTS as readonly string[]).includes(request.type);
}

/** How to undo one queued write of a run. */
interface UndoStep {
  ids: string[];
  undo: (core: MockCore) => Response;
}

/** Mock-core state of this feature. */
export interface RulesMockState {
  /** Rules in file order. */
  rules: Rule[];
  /** Runs, oldest first. */
  runs: RuleRun[];
  /** Undo steps of each run, in the order they were queued. */
  undo: Map<string, UndoStep[]>;
}

/** A sample rule so the mock app shows something. */
export function sampleRule(): Rule {
  return {
    id: "rust-announcements",
    name: "Rust announcements",
    enabled: true,
    triggers: [{ type: "arrival" }],
    scope: [],
    conditions: {
      type: "any",
      conditions: [
        {
          type: "subject",
          matches: { op: "starts-with", value: "[rust-announce]", case_sensitive: false },
        },
        { type: "query", query: "from:lists.example" },
      ],
    },
    actions: [
      { type: "tag", tag: "rust" },
      { type: "notify", notify: false },
    ],
    source: { file: "rules.kdl", line: 1, column: 1 },
  };
}

export function createRulesMockState(): RulesMockState {
  return { rules: [sampleRule()], runs: [], undo: new Map() };
}

function state(core: MockCore): RulesMockState {
  return core.data.features.rules;
}

function locate(rules: Rule[]): Rule[] {
  // Rules are "written" one after another, as the canonical form would.
  let line = 1;
  return rules.map((r) => {
    const located = { ...r, source: { file: "rules.kdl", line, column: 1 } };
    line += ruleToText(r).split("\n").length;
    return located;
  });
}

function invalidRegex(rule: Rule): string | null {
  const walk = (c: Rule["conditions"]): string | null => {
    const texts = (() => {
      switch (c.type) {
        case "header":
        case "list-id":
          return c.matches ? [c.matches] : [];
        case "address":
        case "subject":
        case "body":
          return [c.matches];
        case "attachment":
          return [c.name, c.mime].filter((m) => m !== null);
        default:
          return [];
      }
    })();
    for (const m of texts) {
      if (m.op !== "regex") continue;
      try {
        new RegExp(m.value);
      } catch {
        return m.value;
      }
    }
    const kids =
      c.type === "all" || c.type === "any" ? c.conditions : c.type === "not" ? [c.condition] : [];
    for (const k of kids) {
      const bad = walk(k);
      if (bad !== null) return bad;
    }
    return null;
  };
  return walk(rule.conditions);
}

function saveRule(core: MockCore, rule: Rule, position: number | null): Response {
  const s = state(core);
  if (rule.name.trim() === "") return errorResponse("invalid_request", "a rule needs a name");
  const bad = invalidRegex(rule);
  if (bad !== null) {
    return errorResponse("invalid_request", `condition: invalid regular expression ${bad}`);
  }
  let id = rule.id;
  if (id === "") {
    const base = ruleIdBase(rule.name);
    id = base;
    for (let n = 2; s.rules.some((r) => r.id === id); n++) id = `${base}-${n}`;
  }
  const saved: Rule = { ...rule, id, name: rule.name.trim() };
  const index = s.rules.findIndex((r) => r.id === id);
  const rules = [...s.rules];
  if (index >= 0) {
    rules[index] = saved;
  } else {
    const at = Math.min(position ?? rules.length, rules.length);
    rules.splice(at, 0, saved);
  }
  s.rules = locate(rules);
  queueMicrotask(() => core.emit({ type: "RulesChanged" }));
  return { type: "RuleSaved", rule: s.rules.find((r) => r.id === id) as Rule };
}

/** Messages a rule looks at: a folder, given messages, or its scope (all mail). */
function pool(core: MockCore, folderId: string | null, messageIds: readonly string[]) {
  const d = core.data;
  if (messageIds.length > 0) return d.messages.filter((m) => messageIds.includes(m.id));
  const list = folderId !== null ? folderMessages(d, folderId) : [...d.messages];
  return list?.sort((a, b) => Date.parse(b.date) - Date.parse(a.date)) ?? null;
}

/** The actions that run, up to and including a `stop`. */
function effective(actions: readonly RuleAction[]): RuleAction[] {
  const stop = actions.findIndex((a) => a.type === "stop");
  return stop < 0 ? [...actions] : actions.slice(0, stop + 1);
}

function queued(response: Response): string[] {
  return response.type === "Queued" ? response.op_ids : [];
}

/** Applies one action to one message; returns the op ids and how to undo. */
function apply(
  core: MockCore,
  action: RuleAction,
  m: MessageSummary,
): { ops: string[]; undo: UndoStep | null; irreversible: boolean } {
  const flagWrite = (change: Partial<FlagChange>, undoChange: Partial<FlagChange>) => {
    const full = (c: Partial<FlagChange>): FlagChange => ({
      seen: null,
      flagged: null,
      answered: null,
      draft: null,
      add_tags: [],
      remove_tags: [],
      ...c,
    });
    return {
      ops: queued(updateFlags(core, [m.id], full(change))),
      undo: { ids: [m.id], undo: (c: MockCore) => updateFlags(c, [m.id], full(undoChange)) },
      irreversible: false,
    };
  };
  const moveTo = (folderId: string | undefined) => {
    if (folderId === undefined || folderId === m.folder_id) {
      return { ops: [], undo: null, irreversible: false };
    }
    const back = m.folder_id;
    return {
      ops: queued(moveMessages(core, [m.id], { folder_id: folderId })),
      undo: {
        ids: [m.id],
        undo: (c: MockCore) => moveMessages(c, [m.id], { folder_id: back }),
      },
      irreversible: false,
    };
  };
  const d = core.data;
  switch (action.type) {
    case "tag":
      return m.tags.includes(action.tag)
        ? { ops: [], undo: null, irreversible: false }
        : flagWrite({ add_tags: [action.tag] }, { remove_tags: [action.tag] });
    case "untag":
      return !m.tags.includes(action.tag)
        ? { ops: [], undo: null, irreversible: false }
        : flagWrite({ remove_tags: [action.tag] }, { add_tags: [action.tag] });
    case "flag":
      return m.flags.flagged === action.flagged
        ? { ops: [], undo: null, irreversible: false }
        : flagWrite({ flagged: action.flagged }, { flagged: m.flags.flagged });
    case "mark-read":
      return m.flags.seen === action.read
        ? { ops: [], undo: null, irreversible: false }
        : flagWrite({ seen: action.read }, { seen: m.flags.seen });
    case "move":
      return moveTo(resolveFolder(d, action.folder, m.account_id)?.id);
    case "archive":
      return moveTo(
        resolveFolder(d, { type: "role", role: "archive", account_id: null }, m.account_id)?.id,
      );
    case "delete": {
      const trash = d.folders.find((f) => f.account_id === m.account_id && f.role === "trash");
      const inTrash = trash?.id === m.folder_id;
      const back = m.folder_id;
      const ops = queued(deleteMessages(core, [m.id], action.permanent));
      if (action.permanent || inTrash || !trash) return { ops, undo: null, irreversible: true };
      return {
        ops,
        undo: {
          ids: [m.id],
          undo: (c: MockCore) => moveMessages(c, [m.id], { folder_id: back }),
        },
        irreversible: false,
      };
    }
    case "copy":
    case "forward":
    case "redirect":
    case "reply":
      // The mock sends nothing; these cannot be undone.
      return { ops: [], undo: null, irreversible: true };
    case "notify":
    case "stop":
      return { ops: [], undo: null, irreversible: false };
  }
}

function runRule(
  core: MockCore,
  ruleId: string,
  messageIds: readonly string[],
  folderId: string | null,
): Response {
  const s = state(core);
  const rule = s.rules.find((r) => r.id === ruleId);
  if (!rule) return errorResponse("not_found", `rule ${ruleId} not found`);
  const messages = pool(core, folderId, messageIds);
  if (!messages) return errorResponse("not_found", `folder ${folderId} not found`);
  const d = core.data;
  const matching = messages.filter((m) => explain(d, rule, m).matched);
  const runId = core.nextId("run-");
  const steps: UndoStep[] = [];
  const opIds: string[] = [];
  let irreversible = false;
  for (const summary of matching) {
    for (const action of effective(rule.actions)) {
      // Look the message up again: an earlier action may have moved it.
      const m = d.messages.find((x) => x.id === summary.id);
      if (!m) break;
      const result = apply(core, action, m);
      opIds.push(...result.ops);
      if (result.undo) steps.push(result.undo);
      irreversible ||= result.irreversible;
    }
  }
  const run: RuleRun = {
    id: runId,
    rule_id: rule.id,
    rule_name: rule.name,
    cause: "manual",
    state: "running",
    started_at: new Date(MOCK_NOW).toISOString(),
    finished_at: null,
    matched: matching.length,
    op_ids: opIds,
    undoable: false,
    message: null,
  };
  s.runs.push(run);
  s.undo.set(runId, steps);
  core.later(() => {
    run.state = "done";
    run.finished_at = new Date(MOCK_NOW).toISOString();
    run.undoable = !irreversible && opIds.length > 0;
    core.emit({ type: "RuleRunFinished", run: { ...run } });
  });
  return { type: "RuleRan", run: { ...run } };
}

function undoRun(core: MockCore, runId: string): Response {
  const s = state(core);
  const run = s.runs.find((r) => r.id === runId);
  if (!run) return errorResponse("not_found", `rule run ${runId} not found`);
  if (!run.undoable) return errorResponse("conflict", `rule run ${runId} cannot be undone`);
  const opIds: string[] = [];
  for (const step of [...(s.undo.get(runId) ?? [])].reverse()) {
    if (step.ids.every((id) => core.data.messages.some((m) => m.id === id))) {
      opIds.push(...queued(step.undo(core)));
    }
  }
  run.state = "undone";
  run.undoable = false;
  queueMicrotask(() => core.emit({ type: "RuleRunFinished", run: { ...run } }));
  return { type: "Queued", op_ids: opIds };
}

export function answerRules(core: MockCore, request: RulesRequest): Response {
  const s = state(core);
  switch (request.type) {
    case "ListRules":
      return { type: "Rules", rules: s.rules };
    case "SaveRule":
      return saveRule(core, request.rule, request.position);
    case "DeleteRule": {
      if (!s.rules.some((r) => r.id === request.rule_id)) {
        return errorResponse("not_found", `rule ${request.rule_id} not found`);
      }
      s.rules = locate(s.rules.filter((r) => r.id !== request.rule_id));
      queueMicrotask(() => core.emit({ type: "RulesChanged" }));
      return { type: "Done" };
    }
    case "ReorderRules": {
      const ids = request.rule_ids;
      const complete =
        ids.length === s.rules.length &&
        new Set(ids).size === ids.length &&
        ids.every((id) => s.rules.some((r) => r.id === id));
      if (!complete) {
        return errorResponse("invalid_request", "the new order must list every rule once");
      }
      s.rules = locate(ids.map((id) => s.rules.find((r) => r.id === id) as Rule));
      queueMicrotask(() => core.emit({ type: "RulesChanged" }));
      return { type: "Done" };
    }
    case "TestRule": {
      const bad = invalidRegex(request.rule);
      if (bad !== null) {
        return errorResponse("invalid_request", `condition: invalid regular expression ${bad}`);
      }
      const messages = pool(core, request.folder_id, []);
      if (!messages) return errorResponse("not_found", `folder ${request.folder_id} not found`);
      const matches: ExplainedMatch[] = [];
      let scanned = 0;
      for (const m of messages) {
        if (matches.length >= request.limit) break;
        scanned++;
        const { matched, hits } = explain(core.data, request.rule, m);
        if (matched) {
          matches.push({ message: m, actions: effective(request.rule.actions), explanation: hits });
        }
      }
      return { type: "RuleTested", scanned, matches };
    }
    case "RunRule":
      return runRule(core, request.rule_id, request.message_ids, request.folder_id);
    case "ListRuleRuns":
      return {
        type: "RuleRuns",
        runs: s.runs
          .filter((r) => request.rule_id === null || r.rule_id === request.rule_id)
          .slice()
          .reverse()
          .slice(0, request.limit)
          .map((r) => ({ ...r })),
      };
    case "UndoRuleRun":
      return undoRun(core, request.run_id);
    case "RuleToText":
      return { type: "RuleText", text: ruleToText(request.rule) };
    case "RuleFromText": {
      const { rule, errors } = ruleFromText(request.text);
      return { type: "RuleParsed", rule, errors };
    }
  }
}
