/**
 * The mock core's rule evaluation for `TestRule` / `RunRule`, over the
 * mock mailbox (summaries and bodies; the mock has no raw headers, so
 * header and authentication conditions see none). The real engine is
 * `crates/nail-rules`.
 */
import type {
  Address,
  Condition,
  FolderRef,
  MessageSummary,
  Rule,
  TextMatch,
} from "../../api/generated";
import type { MockData } from "../../api/mockData";
import { MOCK_NOW } from "../../api/mockData";
import { messageSize, searchable } from "../../api/mockMail";
import { matches as queryMatches, parseQuery } from "../../api/mockSearch";
import { comparePaths } from "./model";
import type { ConditionHit } from "./types";
import { DAY_MS } from "../../state/clock";

export function textMatches(m: TextMatch, value: string): boolean {
  if (m.op === "regex") {
    try {
      return new RegExp(m.value, m.case_sensitive ? "" : "i").test(value);
    } catch {
      return false;
    }
  }
  const norm = (s: string) => (m.case_sensitive ? s : s.toLowerCase()).trim();
  const v = norm(value);
  const want = norm(m.value);
  switch (m.op) {
    case "equals":
      return v === want;
    case "contains":
      return v.includes(want);
    case "starts-with":
      return v.startsWith(want);
    case "ends-with":
      return v.endsWith(want);
  }
}

function addressValues(addresses: readonly (Address | null)[]): string[] {
  return addresses.flatMap((a) =>
    a ? [a.email, ...(a.name ? [a.name, `${a.name} <${a.email}>`] : [])] : [],
  );
}

export function resolveFolder(d: MockData, ref: FolderRef, accountId: string) {
  const account = ref.account_id ?? accountId;
  const folders = d.folders.filter((f) => f.account_id === account);
  if (ref.type === "role") return folders.find((f) => f.role === ref.role);
  const wanted = ref.path.replace(/^\/+|\/+$/g, "").toLowerCase();
  return (
    folders.find((f) => f.path.toLowerCase() === wanted) ??
    (wanted === "inbox" ? folders.find((f) => f.role === "inbox") : undefined)
  );
}

function accountMatches(d: MockData, messageAccount: string, wanted: string): boolean {
  if (wanted === messageAccount || wanted === "all") return true;
  const va = d.accountsConfig.virtual_accounts.find((v) => v.id === wanted);
  return va?.members.includes(messageAccount) ?? false;
}

type Decision = [boolean, string | null];

function first(m: TextMatch, values: string[]): Decision {
  const hit = values.find((v) => textMatches(m, v));
  return hit !== undefined ? [true, hit] : [false, values[0] ?? null];
}

function leaf(d: MockData, c: Condition, m: MessageSummary): Decision {
  const body = d.bodies.get(m.id);
  switch (c.type) {
    case "header":
    case "list-id":
      return [false, null];
    case "auth":
      return [c.result === "none", null];
    case "address": {
      const fields = {
        from: [m.from],
        to: m.to,
        cc: body?.headers.cc ?? [],
        recipients: [...m.to, ...(body?.headers.cc ?? [])],
        "reply-to": body?.headers.reply_to ?? [],
        sender: [m.from],
      };
      return first(c.matches, addressValues(fields[c.field]));
    }
    case "subject":
      return first(c.matches, [m.subject]);
    case "body":
      return [textMatches(c.matches, body?.text ?? m.preview), null];
    case "query": {
      const { node } = parseQuery(c.query);
      return [node === null || queryMatches(node, searchable(d, m)), null];
    }
    case "attachment": {
      const found = (body?.attachments ?? []).find(
        (a) =>
          (c.name === null || (a.filename !== null && textMatches(c.name, a.filename))) &&
          (c.mime === null || textMatches(c.mime, a.mime)),
      );
      return [
        found !== undefined || (c.name === null && c.mime === null && m.has_attachments),
        found?.filename ?? null,
      ];
    }
    case "size": {
      const size = messageSize(d, m);
      return [c.cmp === "greater" ? size > c.bytes : size < c.bytes, String(size)];
    }
    case "date": {
      const start = Date.parse(`${c.date}T00:00:00Z`);
      const date = Date.parse(m.date);
      return [c.cmp === "before" ? date < start : date >= start + DAY_MS, null];
    }
    case "age": {
      const age = MOCK_NOW - Date.parse(m.date);
      return [
        c.cmp === "greater" ? age > c.days * DAY_MS : age < c.days * DAY_MS,
        String(Math.floor(age / DAY_MS)),
      ];
    }
    case "flag":
      return [m.flags[c.flag] === c.set, null];
    case "tag": {
      const tag = m.tags.find((t) => t.toLowerCase() === c.tag.toLowerCase());
      return [tag !== undefined, tag ?? null];
    }
    case "account":
      return [accountMatches(d, m.account_id, c.account_id), m.account_id];
    case "folder": {
      const folder = resolveFolder(d, c.folder, m.account_id);
      return [
        folder?.id === m.folder_id,
        d.folders.find((f) => f.id === m.folder_id)?.path ?? null,
      ];
    }
    default:
      return [false, null];
  }
}

/** Evaluates a condition, recording every evaluated condition in `hits`. */
export function evaluate(
  d: MockData,
  c: Condition,
  m: MessageSummary,
  path: number[] = [],
  hits: ConditionHit[] = [],
): boolean {
  let result: Decision;
  switch (c.type) {
    case "all":
    case "any": {
      const results = c.conditions.map((child, i) => evaluate(d, child, m, [...path, i], hits));
      result = [c.type === "all" ? results.every(Boolean) : results.some(Boolean), null];
      break;
    }
    case "not":
      result = [!evaluate(d, c.condition, m, [...path, 0], hits), null];
      break;
    default:
      result = leaf(d, c, m);
  }
  hits.push({ path, matched: result[0], value: result[1] });
  return result[0];
}

/** Whether the message is in the rule's scope. */
export function inScope(d: MockData, rule: Rule, m: MessageSummary): boolean {
  return (
    rule.scope.length === 0 ||
    rule.scope.some((s) =>
      s.type === "account"
        ? accountMatches(d, m.account_id, s.account_id)
        : resolveFolder(d, s.folder, m.account_id)?.id === m.folder_id,
    )
  );
}

/** Whether the rule matches a message, with the explanation. */
export function explain(
  d: MockData,
  rule: Rule,
  m: MessageSummary,
): { matched: boolean; hits: ConditionHit[] } {
  if (!inScope(d, rule, m)) return { matched: false, hits: [] };
  const hits: ConditionHit[] = [];
  const matched = evaluate(d, rule.conditions, m, [], hits);
  hits.sort((a, b) => comparePaths(a.path, b.path));
  return { matched, hits };
}
