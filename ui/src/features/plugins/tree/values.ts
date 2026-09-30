/**
 * Values of plug-in UI trees (ADR 0050, `PluginUiNode::value_kind`):
 *
 * | Node | Value |
 * | --- | --- |
 * | `input`, `textarea` | string |
 * | `select` | string (`multiple`: array of strings) |
 * | `checkbox` | boolean |
 * | `date` | `"YYYY-MM-DD"` (`time`: `"YYYY-MM-DDTHH:MM"`), or `null` |
 * | `contact-picker` | array of `{name, email}` |
 * | `folder-picker` | folder id or `null` |
 * | `message-list` (`selectable`) | array of message ids |
 * | `table` (`selectable`) | array of row indexes |
 *
 * plus the checks a primary dialog button runs before it submits.
 */
import type { Address, JsonValue, PluginUiNode } from "../../../api/generated";

export type Values = Record<string, JsonValue>;

/** Why a value is not accepted (message `plugin-field-<issue>`). */
export type FieldIssue = "required" | "email" | "url" | "number" | "too-long" | "date-range";

export const FIELD_ISSUES: readonly FieldIssue[] = [
  "required",
  "email",
  "url",
  "number",
  "too-long",
  "date-range",
];

/** Every node of a tree, depth first. */
export function flatten(nodes: readonly PluginUiNode[]): PluginUiNode[] {
  const out: PluginUiNode[] = [];
  const visit = (list: readonly PluginUiNode[]) => {
    for (const node of list) {
      out.push(node);
      if (node.type === "stack") visit(node.children);
    }
  };
  visit(nodes);
  return out;
}

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

/** `YYYY-MM-DD` of a local date. */
export function isoDate(date: Date): string {
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** The time a `today` / `tomorrow` default gets when a time is picked too. */
export const DEFAULT_TIME = "09:00";

/** A date node's initial value (`today` / `tomorrow` resolved against `now`). */
export function dateDefault(
  value: string | undefined,
  time: boolean | undefined,
  now: Date,
): string | null {
  if (value === undefined) return null;
  if (value === "today" || value === "tomorrow") {
    const day = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    if (value === "tomorrow") day.setDate(day.getDate() + 1);
    const date = isoDate(day);
    return time ? `${date}T${DEFAULT_TIME}` : date;
  }
  if (time && /^\d{4}-\d{2}-\d{2}$/.test(value)) return `${value}T${DEFAULT_TIME}`;
  if (!time && value.length > 10) return value.slice(0, 10);
  return value;
}

/** The value a node starts with; `undefined` for nodes without a value. */
export function initialValue(node: PluginUiNode, now: Date): JsonValue | undefined {
  switch (node.type) {
    case "input":
    case "textarea":
      return node.default ?? "";
    case "select":
      return node.multiple ? [...(node.default ?? [])] : (node.default?.[0] ?? "");
    case "checkbox":
      return node.default ?? false;
    case "date":
      return dateDefault(node.default, node.time, now);
    case "contact-picker":
      return (node.default ?? []).map((a) => ({ name: a.name, email: a.email }));
    case "folder-picker":
      return node.default ?? null;
    case "message-list":
      return node.selectable && node.id !== undefined ? [] : undefined;
    case "table":
      return node.selectable && node.id !== undefined ? [] : undefined;
    default:
      return undefined;
  }
}

/** The id under which a node reports its value, if it has one. */
export function valueId(node: PluginUiNode): string | null {
  switch (node.type) {
    case "input":
    case "textarea":
    case "select":
    case "checkbox":
    case "date":
    case "contact-picker":
    case "folder-picker":
      return node.id;
    case "message-list":
    case "table":
      return node.selectable && node.id !== undefined ? node.id : null;
    default:
      return null;
  }
}

/** The initial values of a tree. */
export function initialValues(nodes: readonly PluginUiNode[], now: Date): Values {
  const values: Values = {};
  for (const node of flatten(nodes)) {
    const id = valueId(node);
    const value = initialValue(node, now);
    if (id !== null && value !== undefined) values[id] = value;
  }
  return values;
}

/**
 * The values for a changed tree (a dialog or panel re-rendered by its
 * plug-in): what the user entered is kept for ids that still exist with a
 * value of the same kind; new ids start with their defaults.
 */
export function mergeValues(nodes: readonly PluginUiNode[], previous: Values, now: Date): Values {
  const next = initialValues(nodes, now);
  for (const [id, value] of Object.entries(next)) {
    const old = previous[id];
    if (old === undefined) continue;
    const sameKind =
      Array.isArray(old) === Array.isArray(value) &&
      (old === null || value === null || typeof old === typeof value);
    if (sameKind) next[id] = old;
  }
  return next;
}

const EMAIL = /^[^\s@<>()",;]+@[^\s@<>()",;]+\.[^\s@<>()",;]+$/;

/** Whether `text` looks like a mail address. */
export function isEmail(text: string): boolean {
  return EMAIL.test(text);
}

function isUrl(text: string): boolean {
  try {
    const url = new URL(text);
    return url.protocol === "https:" || url.protocol === "http:";
  } catch {
    return false;
  }
}

function isEmpty(value: JsonValue | undefined): boolean {
  return (
    value === undefined ||
    value === null ||
    value === "" ||
    value === false ||
    (Array.isArray(value) && value.length === 0)
  );
}

function length(text: string): number {
  return [...text].length;
}

/** The problem with one node's value, or `null`. */
export function checkNode(node: PluginUiNode, value: JsonValue | undefined): FieldIssue | null {
  switch (node.type) {
    case "input": {
      const text = typeof value === "string" ? value : "";
      if (text.trim() === "") return node.required ? "required" : null;
      if (node.maxLength !== undefined && length(text) > node.maxLength) return "too-long";
      if (node.kind === "email" && !isEmail(text.trim())) return "email";
      if (node.kind === "url" && !isUrl(text.trim())) return "url";
      if (node.kind === "number" && !Number.isFinite(Number(text.trim()))) return "number";
      return null;
    }
    case "textarea": {
      const text = typeof value === "string" ? value : "";
      if (text.trim() === "") return node.required ? "required" : null;
      if (node.maxLength !== undefined && length(text) > node.maxLength) return "too-long";
      return null;
    }
    case "select":
    case "folder-picker":
    case "contact-picker":
      return node.required && isEmpty(value) ? "required" : null;
    case "date": {
      if (typeof value !== "string" || value === "") return node.required ? "required" : null;
      const day = value.slice(0, 10);
      if ((node.min && day < node.min) || (node.max && day > node.max)) return "date-range";
      return null;
    }
    default:
      return null;
  }
}

/** Problems of a tree's values by node id (empty = everything valid). */
export function validate(
  nodes: readonly PluginUiNode[],
  values: Values,
): Record<string, FieldIssue> {
  const issues: Record<string, FieldIssue> = {};
  for (const node of flatten(nodes)) {
    const id = valueId(node);
    if (id === null) continue;
    const issue = checkNode(node, values[id]);
    if (issue) issues[id] = issue;
  }
  return issues;
}

/** Parses typed addresses: `Ada <ada@example.org>`, `ada@example.org`. */
export function parseAddress(text: string): Address | null {
  const trimmed = text.trim();
  const angled = /^(.*?)\s*<([^<>]+)>$/.exec(trimmed);
  if (angled) {
    const email = (angled[2] ?? "").trim();
    const name = (angled[1] ?? "").trim().replace(/^"(.*)"$/, "$1");
    return isEmail(email) ? { name: name === "" ? null : name, email } : null;
  }
  return isEmail(trimmed) ? { name: null, email: trimmed } : null;
}
