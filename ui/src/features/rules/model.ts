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
export { TEXT_OP_VALUES as TEXT_OPS } from "../../api/generated";

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

/** The condition kinds that are groups (listed last in "+ Add condition"). */
export const GROUP_KINDS = ["all", "any", "not"] as const satisfies readonly ConditionKind[];

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

/**
 * Where a problem is, so the editor marks that field (`aria-invalid`):
 * the name, a condition by its path, an action by its index, a trigger.
 */
export type IssueAt =
  | { kind: "name" }
  | { kind: "condition"; path: ConditionPath }
  | { kind: "action"; index: number }
  | { kind: "trigger"; trigger: TriggerKind };

/** One problem that keeps a rule from being saved: its Fluent id and its field. */
export interface RuleIssue {
  id: string;
  at: IssueAt;
}

/** Whether a (non-group) condition still misses its value. */
function conditionIncomplete(c: Condition): boolean {
  switch (c.type) {
    case "header":
      return c.name.trim() === "";
    case "address":
    case "subject":
    case "body":
      return c.matches.value === "";
    case "query":
      return c.query.trim() === "";
    case "date":
      return !/^\d{4}-\d{2}-\d{2}$/.test(c.date);
    case "tag":
      return c.tag.trim() === "";
    case "account":
      return c.account_id === "";
    case "folder":
      return c.folder.type === "path" && c.folder.path.trim() === "";
    default:
      return false;
  }
}

/** Whether an action still misses its value. */
function actionIncomplete(a: RuleAction): boolean {
  switch (a.type) {
    case "move":
    case "copy":
      return a.folder.type === "path" && a.folder.path.trim() === "";
    case "tag":
    case "untag":
      return a.tag.trim() === "";
    case "forward":
    case "redirect":
      return a.to.length === 0 || a.to.some((t) => !t.includes("@"));
    case "reply":
      return a.template.trim() === "";
    default:
      return false;
  }
}

/** Every problem that keeps a rule from being saved, with the field it is in. */
export function ruleIssues(rule: Rule): RuleIssue[] {
  const issues: RuleIssue[] = [];
  if (rule.name.trim() === "") issues.push({ id: "rules-error-name", at: { kind: "name" } });
  if (rule.actions.length === 0) {
    issues.push({ id: "rules-error-no-actions", at: { kind: "action", index: -1 } });
  }
  const walk = (c: Condition, path: ConditionPath) => {
    if (conditionIncomplete(c)) {
      issues.push({ id: "rules-error-condition", at: { kind: "condition", path } });
    }
    childrenOf(c).forEach((child, i) => {
      walk(child, [...path, i]);
    });
  };
  walk(rule.conditions, []);
  rule.actions.forEach((a, index) => {
    if (actionIncomplete(a))
      issues.push({ id: "rules-error-action", at: { kind: "action", index } });
  });
  for (const t of rule.triggers) {
    if (
      (t.type === "schedule" && t.interval_minutes < 1) ||
      (t.type === "older-than" && t.days < 1)
    ) {
      issues.push({ id: "rules-error-trigger", at: { kind: "trigger", trigger: t.type } });
    }
  }
  return issues;
}

/** Problems that keep a rule from being saved (Fluent ids, each once). */
export function ruleProblems(rule: Rule): string[] {
  return [...new Set(ruleIssues(rule).map((i) => i.id))];
}

/** Whether `issues` has one at the condition `path`. */
export function conditionInvalid(issues: readonly RuleIssue[], path: ConditionPath): boolean {
  return issues.some((i) => i.at.kind === "condition" && comparePaths(i.at.path, path) === 0);
}

/** Whether `issues` has one at the action `index`. */
export function actionInvalid(issues: readonly RuleIssue[], index: number): boolean {
  return issues.some((i) => i.at.kind === "action" && i.at.index === index);
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

/**
 * Whether running the rule can take mail away from where it is (it moves,
 * archives or deletes): such a run asks first, with the number of messages.
 */
export function isDestructive(rule: Rule): boolean {
  return rule.actions.some((a) => a.type === "move" || a.type === "archive" || a.type === "delete");
}

/** The rules whose name contains `text` (case-insensitive); all for empty text. */
export function filterRules<R extends Pick<Rule, "name">>(rules: readonly R[], text: string): R[] {
  const needle = text.trim().toLocaleLowerCase();
  if (needle === "") return [...rules];
  return rules.filter((r) => r.name.toLocaleLowerCase().includes(needle));
}

/** Moves the item at `from` to `to` (drag and drop); unchanged when either is out of range. */
export function moveItemTo<T>(items: readonly T[], from: number, to: number): T[] {
  if (from < 0 || from >= items.length || to < 0 || to >= items.length) return [...items];
  const next = [...items];
  const [item] = next.splice(from, 1);
  next.splice(to, 0, item as T);
  return next;
}
