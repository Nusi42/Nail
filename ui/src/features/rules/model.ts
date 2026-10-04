/**
 * Pure helpers for editing rules in the UI: blank conditions and actions
 * of each kind, edits of the nested condition tree by path, problems that
 * keep a rule from being saved, and a rule pre-filled from a message.
 * The core validates again (regular expressions, queries) when saving.
 */
import type {
  Condition,
  FolderRef,
  MessageSummary,
  Rule,
  RuleAction,
  RuleTrigger,
  TextMatch,
  TextOp,
} from "../../api/generated";
import { nonEmpty } from "../../text/empty";

export type ConditionKind = Condition["type"];
export type ActionKind = RuleAction["type"];
export type TriggerKind = RuleTrigger["type"];
/** Child indexes from the root condition. */
export type ConditionPath = readonly number[];

/** Condition kinds offered in the editor, in menu order (groups last). */
export const CONDITION_KINDS: readonly ConditionKind[] = [
  "address",
  "subject",
  "body",
  "header",
  "query",
  "attachment",
  "size",
  "date",
  "age",
  "flag",
  "tag",
  "account",
  "folder",
  "auth",
  "list-id",
  "all",
  "any",
  "not",
];

/** Action kinds offered in the editor, in menu order. */
export const ACTION_KINDS: readonly ActionKind[] = [
  "move",
  "copy",
  "archive",
  "delete",
  "tag",
  "untag",
  "flag",
  "mark-read",
  "forward",
  "redirect",
  "reply",
  "notify",
  "stop",
];

export const TEXT_OPS: readonly TextOp[] = [
  "contains",
  "equals",
  "starts-with",
  "ends-with",
  "regex",
];

export const TRIGGER_KINDS: readonly TriggerKind[] = [
  "arrival",
  "on-send",
  "manual",
  "schedule",
  "older-than",
];

export function textMatch(value = "", op: TextOp = "contains"): TextMatch {
  return { op, value, case_sensitive: false };
}

export function inboxRef(): FolderRef {
  return { type: "path", path: "", account_id: null };
}

/** A blank condition of a kind. */
export function newCondition(kind: ConditionKind): Condition {
  switch (kind) {
    case "all":
    case "any":
      return { type: kind, conditions: [] };
    case "not":
      return { type: "not", condition: newCondition("subject") };
    case "header":
      return { type: "header", name: "", matches: null };
    case "address":
      return { type: "address", field: "from", matches: textMatch() };
    case "subject":
    case "body":
      return { type: kind, matches: textMatch() };
    case "query":
      return { type: "query", query: "" };
    case "attachment":
      return { type: "attachment", name: null, mime: null };
    case "size":
      return { type: "size", cmp: "greater", bytes: 1_048_576 };
    case "date":
      return { type: "date", cmp: "before", date: "" };
    case "age":
      return { type: "age", cmp: "greater", days: 30 };
    case "flag":
      return { type: "flag", flag: "flagged", set: true };
    case "tag":
      return { type: "tag", tag: "" };
    case "account":
      return { type: "account", account_id: "" };
    case "folder":
      return { type: "folder", folder: inboxRef() };
    case "auth":
      return { type: "auth", method: "dkim", result: "fail" };
    case "list-id":
      return { type: "list-id", matches: null };
  }
}

/** A blank action of a kind. */
export function newAction(kind: ActionKind): RuleAction {
  switch (kind) {
    case "move":
    case "copy":
      return { type: kind, folder: inboxRef() };
    case "archive":
    case "stop":
      return { type: kind };
    case "delete":
      return { type: "delete", permanent: false };
    case "tag":
    case "untag":
      return { type: kind, tag: "" };
    case "flag":
      return { type: "flag", flagged: true };
    case "mark-read":
      return { type: "mark-read", read: true };
    case "forward":
      return { type: "forward", to: [], identity_id: null };
    case "redirect":
      return { type: "redirect", to: [] };
    case "reply":
      return { type: "reply", template: "", identity_id: null };
    case "notify":
      return { type: "notify", notify: false };
  }
}

/** A new, empty rule (runs on arrival). */
export function newRule(): Rule {
  return {
    id: "",
    name: "",
    enabled: true,
    triggers: [{ type: "arrival" }],
    scope: [],
    conditions: { type: "all", conditions: [] },
    actions: [],
    source: null,
  };
}

/** The children of a group condition (`not` has one). */
export function childrenOf(condition: Condition): Condition[] {
  switch (condition.type) {
    case "all":
    case "any":
      return condition.conditions;
    case "not":
      return [condition.condition];
    default:
      return [];
  }
}

export function isGroup(condition: Condition): boolean {
  return condition.type === "all" || condition.type === "any" || condition.type === "not";
}

/** The condition at `path`, or `undefined`. */
export function conditionAt(root: Condition, path: ConditionPath): Condition | undefined {
  let node: Condition | undefined = root;
  for (const index of path) {
    node = node ? childrenOf(node)[index] : undefined;
  }
  return node;
}

function withChildren(group: Condition, children: Condition[]): Condition {
  switch (group.type) {
    case "all":
    case "any":
      return { ...group, conditions: children };
    case "not":
      return children[0] ? { type: "not", condition: children[0] } : group;
    default:
      return group;
  }
}

/** Replaces the condition at `path` (the root when the path is empty). */
export function replaceAt(root: Condition, path: ConditionPath, next: Condition): Condition {
  if (path.length === 0) return next;
  const [head, ...rest] = path as [number, ...number[]];
  const children = childrenOf(root);
  const child = children[head];
  if (child === undefined) return root;
  return withChildren(
    root,
    children.map((c, i) => (i === head ? replaceAt(child, rest, next) : c)),
  );
}

/** Appends a child to the group at `path`. A `not` gets its child replaced. */
export function addChild(root: Condition, path: ConditionPath, child: Condition): Condition {
  const group = conditionAt(root, path);
  if (!group || !isGroup(group)) return root;
  const children = group.type === "not" ? [child] : [...childrenOf(group), child];
  return replaceAt(root, path, withChildren(group, children));
}

/**
 * Removes the condition at `path` (not the root). Removing the only child
 * of a `not` removes the `not` itself.
 */
export function removeAt(root: Condition, path: ConditionPath): Condition {
  const index = path.at(-1);
  if (index === undefined) return root;
  const parentPath = path.slice(0, -1);
  const parent = conditionAt(root, parentPath);
  if (!parent) return root;
  if (parent.type === "not") return removeAt(root, parentPath);
  return replaceAt(
    root,
    parentPath,
    withChildren(
      parent,
      childrenOf(parent).filter((_, i) => i !== index),
    ),
  );
}

/** Changes a group between all / any, or wraps a condition into `not`. */
export function setGroupMode(root: Condition, path: ConditionPath, mode: "all" | "any"): Condition {
  const group = conditionAt(root, path);
  if (!group || (group.type !== "all" && group.type !== "any")) return root;
  return replaceAt(root, path, { type: mode, conditions: group.conditions });
}

/** Tree order of condition paths: a group before its children, children by index. */
export function comparePaths(a: readonly number[], b: readonly number[]): number {
  for (let i = 0; ; i++) {
    const x = a[i];
    const y = b[i];
    if (x === undefined || y === undefined) return a.length - b.length;
    if (x !== y) return x - y;
  }
}

/** Swaps an item with its neighbour; returns the list unchanged at the ends. */
export function moveItem<T>(items: readonly T[], index: number, delta: -1 | 1): T[] {
  const target = index + delta;
  if (target < 0 || target >= items.length) return [...items];
  const next = [...items];
  [next[index], next[target]] = [next[target] as T, next[index] as T];
  return next;
}

/** Problems that keep a rule from being saved (Fluent ids). */
export function ruleProblems(rule: Rule): string[] {
  const problems = new Set<string>();
  if (rule.name.trim() === "") problems.add("rules-error-name");
  if (rule.actions.length === 0) problems.add("rules-error-no-actions");
  const walk = (c: Condition) => {
    switch (c.type) {
      case "header":
        if (c.name.trim() === "") problems.add("rules-error-condition");
        break;
      case "address":
      case "subject":
      case "body":
        if (c.matches.value === "") problems.add("rules-error-condition");
        break;
      case "query":
        if (c.query.trim() === "") problems.add("rules-error-condition");
        break;
      case "date":
        if (!/^\d{4}-\d{2}-\d{2}$/.test(c.date)) problems.add("rules-error-condition");
        break;
      case "tag":
        if (c.tag.trim() === "") problems.add("rules-error-condition");
        break;
      case "account":
        if (c.account_id === "") problems.add("rules-error-condition");
        break;
      case "folder":
        if (c.folder.type === "path" && c.folder.path.trim() === "") {
          problems.add("rules-error-condition");
        }
        break;
      default:
        break;
    }
    childrenOf(c).forEach(walk);
  };
  walk(rule.conditions);
  for (const a of rule.actions) {
    switch (a.type) {
      case "move":
      case "copy":
        if (a.folder.type === "path" && a.folder.path.trim() === "") {
          problems.add("rules-error-action");
        }
        break;
      case "tag":
      case "untag":
        if (a.tag.trim() === "") problems.add("rules-error-action");
        break;
      case "forward":
      case "redirect":
        if (a.to.length === 0 || a.to.some((t) => !t.includes("@"))) {
          problems.add("rules-error-action");
        }
        break;
      case "reply":
        if (a.template.trim() === "") problems.add("rules-error-action");
        break;
      default:
        break;
    }
  }
  for (const t of rule.triggers) {
    if (
      (t.type === "schedule" && t.interval_minutes < 1) ||
      (t.type === "older-than" && t.days < 1)
    ) {
      problems.add("rules-error-trigger");
    }
  }
  return [...problems];
}

/** Adds or removes a trigger kind (with default parameters). */
export function toggleTrigger(rule: Rule, kind: TriggerKind, on: boolean): Rule {
  const rest = rule.triggers.filter((t) => t.type !== kind);
  if (!on) return { ...rule, triggers: rest };
  const trigger: RuleTrigger =
    kind === "schedule"
      ? { type: "schedule", interval_minutes: 60 }
      : kind === "older-than"
        ? { type: "older-than", days: 30 }
        : { type: kind };
  return { ...rule, triggers: [...rest, trigger] };
}

/** Parses a comma / whitespace separated address list. */
export function parseAddresses(text: string): string[] {
  return text
    .split(/[\s,;]+/)
    .map((s) => s.trim())
    .filter(Boolean);
}

/**
 * "Create rule from message": a rule for mail from the same sender (or,
 * for mailing lists, from the same list address), not saved yet. The user
 * picks the actions.
 */
export function ruleFromMessage(message: MessageSummary): Rule {
  const from = message.from?.email ?? "";
  const listLike = /^\[[^\]]+\]/.exec(message.subject);
  const conditions: Condition[] = [];
  if (from !== "") {
    conditions.push({ type: "address", field: "from", matches: textMatch(from, "equals") });
  }
  if (listLike) {
    conditions.push({ type: "subject", matches: textMatch(listLike[0], "starts-with") });
  }
  if (conditions.length === 0) {
    conditions.push({ type: "subject", matches: textMatch(message.subject, "equals") });
  }
  return {
    ...newRule(),
    name: nonEmpty(message.from?.name) ?? nonEmpty(from) ?? message.subject,
    conditions: { type: "all", conditions },
  };
}
