/**
 * The mock core's `RuleToText` / `RuleFromText`: the `rules.kdl` syntax of
 * one rule, rendered and read like `nail-config` does (canonical form, a
 * KDL subset, the same node and property names). Only `MockTransport`
 * uses this; the real core answers with nail-config's reader and writer.
 */
import { ADDRESS_FIELD_VALUES, RULE_SIMPLE_TRIGGERS, TEXT_OP_VALUES } from "../../api/generated";
import type {
  Condition,
  ConfigIssue,
  FolderRef,
  FolderRole,
  Rule,
  RuleAction,
  RuleScope,
  RuleTrigger,
  TextMatch,
  TextOp,
} from "../../api/generated";

// ---------------------------------------------------------------------------
// Rendering

function quote(s: string): string {
  return `"${s.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n").replace(/\r/g, "\\r").replace(/\t/g, "\\t")}"`;
}

function slug(name: string): string {
  const s = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 60)
    .replace(/-+$/, "");
  return s === "" ? "rule" : s;
}

/** The id a rule gets from its name (`rule_id_base`). */
export function ruleIdBase(name: string): string {
  return slug(name);
}

function textProps(m: TextMatch): string {
  return ` ${m.op}=${quote(m.value)}${m.case_sensitive ? " case-sensitive=#true" : ""}`;
}

function folderLine(name: string, f: FolderRef): string {
  const account = f.account_id ? ` account=${quote(f.account_id)}` : "";
  return f.type === "path"
    ? `${name} ${quote(f.path)}${account}`
    : `${name} role=${quote(f.role)}${account}`;
}

function block(pad: string, head: string, lines: string[]): string {
  return lines.length === 0
    ? `${pad}${head} {\n${pad}}\n`
    : `${pad}${head} {\n${lines.join("")}${pad}}\n`;
}

const IND = "    ";

function renderCondition(c: Condition, depth: number): string {
  const pad = IND.repeat(depth);
  const leaf = (s: string) => `${pad}${s}\n`;
  switch (c.type) {
    case "all":
    case "any":
      return block(
        pad,
        c.type,
        c.conditions.map((x) => renderCondition(x, depth + 1)),
      );
    case "not":
      if (c.condition.type === "all" && c.condition.conditions.length !== 1) {
        return block(
          pad,
          "not",
          c.condition.conditions.map((x) => renderCondition(x, depth + 1)),
        );
      }
      return block(pad, "not", [renderCondition(c.condition, depth + 1)]);
    case "header":
      return leaf(`header ${quote(c.name)}${c.matches ? textProps(c.matches) : " exists=#true"}`);
    case "address":
      return leaf(`${c.field}${textProps(c.matches)}`);
    case "subject":
    case "body":
      return leaf(`${c.type}${textProps(c.matches)}`);
    case "query":
      return leaf(`query ${quote(c.query)}`);
    case "attachment": {
      const kids: string[] = [];
      if (c.name) kids.push(`${IND.repeat(depth + 1)}name${textProps(c.name)}\n`);
      if (c.mime) kids.push(`${IND.repeat(depth + 1)}type${textProps(c.mime)}\n`);
      return kids.length === 0 ? leaf("attachment") : block(pad, "attachment", kids);
    }
    case "size":
      return leaf(`size ${c.cmp === "greater" ? "larger" : "smaller"}=${String(c.bytes)}`);
    case "date":
      return leaf(`date ${c.cmp}=${quote(c.date)}`);
    case "age":
      return leaf(`age ${c.cmp === "greater" ? "older-than" : "newer-than"}=${String(c.days)}`);
    case "flag":
      return leaf(`flag ${quote(c.flag)}${c.set ? "" : " set=#false"}`);
    case "tag":
      return leaf(`tag ${quote(c.tag)}`);
    case "account":
      return leaf(`account ${quote(c.account_id)}`);
    case "folder":
      return leaf(folderLine("folder", c.folder));
    case "auth":
      return leaf(`${c.method} ${quote(c.result)}`);
    case "list-id":
      return leaf(`list-id${c.matches ? textProps(c.matches) : ""}`);
  }
}

function renderAction(a: RuleAction): string {
  const pad = IND.repeat(2);
  const bool = (name: string, v: boolean) => `${name}${v ? "" : " #false"}`;
  const line = (() => {
    switch (a.type) {
      case "move":
      case "copy":
        return folderLine(a.type, a.folder);
      case "delete":
        return a.permanent ? "delete permanent=#true" : "delete";
      case "archive":
      case "stop":
        return a.type;
      case "tag":
      case "untag":
        return `${a.type} ${quote(a.tag)}`;
      case "flag":
        return bool("flag", a.flagged);
      case "mark-read":
        return bool("mark-read", a.read);
      case "notify":
        return bool("notify", a.notify);
      case "forward":
        return `forward${a.to.map((t) => ` ${quote(t)}`).join("")}${a.identity_id ? ` identity=${quote(a.identity_id)}` : ""}`;
      case "redirect":
        return `redirect${a.to.map((t) => ` ${quote(t)}`).join("")}`;
      case "reply":
        return `reply ${quote(a.template)}${a.identity_id ? ` identity=${quote(a.identity_id)}` : ""}`;
    }
  })();
  return `${pad}${line}\n`;
}

function scopeSpec(s: RuleScope): [string, string | null] {
  if (s.type === "account") return [`account:${s.account_id}`, null];
  return s.folder.type === "path"
    ? [`folder:${s.folder.path}`, s.folder.account_id]
    : [`role:${s.folder.role}`, s.folder.account_id];
}

/** The canonical text form of one rule (like `nail_config::rules::rule_to_text`). */
export function ruleToText(rule: Rule): string {
  let head = `rule ${quote(rule.name.trim())}`;
  if (rule.id !== "" && rule.id !== slug(rule.name)) head += ` id=${quote(rule.id)}`;
  if (!rule.enabled) head += " enabled=#false";
  const children: string[] = [];
  const [onlyTrigger] = rule.triggers;
  if (
    rule.triggers.length === 1 &&
    onlyTrigger &&
    (RULE_SIMPLE_TRIGGERS as readonly string[]).includes(onlyTrigger.type)
  ) {
    head += ` trigger=${quote(onlyTrigger.type)}`;
  } else {
    for (const t of rule.triggers) {
      children.push(
        t.type === "schedule"
          ? `${IND}trigger "schedule" every=${String(t.interval_minutes)}\n`
          : t.type === "older-than"
            ? `${IND}trigger "older-than" days=${String(t.days)}\n`
            : `${IND}trigger ${quote(t.type)}\n`,
      );
    }
  }
  const [onlyScope] = rule.scope;
  if (rule.scope.length === 1 && onlyScope?.type === "account") {
    head += ` scope=${quote(scopeSpec(onlyScope)[0])}`;
  } else {
    for (const s of rule.scope) {
      const [spec, account] = scopeSpec(s);
      children.push(`${IND}scope ${quote(spec)}${account ? ` account=${quote(account)}` : ""}\n`);
    }
  }
  const root = rule.conditions;
  const mode = root.type === "any" ? " any" : "";
  const items = root.type === "all" || root.type === "any" ? root.conditions : [root];
  if (mode !== "" || items.length > 0) {
    children.push(
      block(
        IND,
        `when${mode}`,
        items.map((c) => renderCondition(c, 2)),
      ),
    );
  }
  if (rule.actions.length > 0) children.push(block(IND, "then", rule.actions.map(renderAction)));
  return children.length === 0 ? `${head} {\n}\n` : block("", head, children);
}

// ---------------------------------------------------------------------------
// Reading

type Value = string | number | boolean | null;

interface Entry {
  name: string | null;
  value: Value;
  line: number;
  column: number;
}

interface Node {
  name: string;
  entries: Entry[];
  children: Node[];
  line: number;
  column: number;
}

class Issue extends Error {
  constructor(
    readonly line: number,
    readonly column: number,
    message: string,
  ) {
    super(message);
  }
}

/** A small KDL v2 reader: nodes, arguments, properties, children, comments. */
function parseKdl(text: string): Node[] {
  let i = 0;
  let line = 1;
  let col = 1;
  const peek = (k = 0) => text[i + k] ?? "";
  const advance = () => {
    if (text[i] === "\n") {
      line++;
      col = 1;
    } else {
      col++;
    }
    i++;
  };
  const fail = (message: string): never => {
    throw new Issue(line, col, message);
  };
  const skipSpace = (newlines: boolean) => {
    for (;;) {
      const c = peek();
      if (c === " " || c === "\t" || c === "\r" || (newlines && (c === "\n" || c === ";"))) {
        advance();
      } else if (c === "/" && peek(1) === "/") {
        while (i < text.length && peek() !== "\n") advance();
      } else if (c === "/" && peek(1) === "*") {
        while (i < text.length && !(peek() === "*" && peek(1) === "/")) advance();
        advance();
        advance();
      } else if (c === "\\" && !newlines) {
        advance();
        while (i < text.length && peek() !== "\n") advance();
        advance();
      } else {
        return;
      }
    }
  };
  const isWordChar = (c: string) => c !== "" && !/[\s{}();=\\"/#[\]]/.test(c);
  const readString = (): string => {
    advance(); // opening quote
    let out = "";
    for (;;) {
      const c = peek();
      if (c === "") fail("syntax error: unterminated string");
      if (c === '"') {
        advance();
        return out;
      }
      if (c === "\\") {
        advance();
        const e = peek();
        advance();
        if (e === "n") out += "\n";
        else if (e === "t") out += "\t";
        else if (e === "r") out += "\r";
        else if (e === "u" && peek() === "{") {
          advance();
          let hex = "";
          while (peek() !== "}" && peek() !== "") {
            hex += peek();
            advance();
          }
          advance();
          out += String.fromCodePoint(parseInt(hex, 16));
        } else out += e;
      } else {
        out += c;
        advance();
      }
    }
  };
  const readValue = (): Value => {
    const c = peek();
    if (c === '"') return readString();
    if (c === "#") {
      let word = "";
      while (isWordChar(peek()) || peek() === "#") {
        word += peek();
        advance();
      }
      if (word === "#true") return true;
      if (word === "#false") return false;
      if (word === "#null") return null;
      return fail(`syntax error: unknown keyword ${word}`);
    }
    let word = "";
    while (isWordChar(peek())) {
      word += peek();
      advance();
    }
    if (word === "") return fail("syntax error: expected a value");
    return /^-?\d+$/.test(word) ? Number(word) : word;
  };
  const readNodes = (nested: boolean): Node[] => {
    const nodes: Node[] = [];
    for (;;) {
      skipSpace(true);
      if (peek() === "") {
        if (nested) fail("syntax error: missing `}`");
        return nodes;
      }
      if (peek() === "}") {
        if (!nested) fail("syntax error: unexpected `}`");
        advance();
        return nodes;
      }
      const node: Node = { name: "", entries: [], children: [], line, column: col };
      const name = readValue();
      if (typeof name !== "string") fail("syntax error: a node name must be a string");
      node.name = name as string;
      for (;;) {
        skipSpace(false);
        const c = peek();
        if (c === "" || c === "\n" || c === ";" || c === "}") break;
        if (c === "{") {
          advance();
          node.children = readNodes(true);
          continue;
        }
        const at = { line, column: col };
        const v = readValue();
        if (peek() === "=" && typeof v === "string") {
          advance();
          node.entries.push({ name: v, value: readValue(), ...at });
        } else {
          node.entries.push({ name: null, value: v, ...at });
        }
      }
      nodes.push(node);
    }
  };
  return readNodes(false);
}

const args = (n: Node) => n.entries.filter((e) => e.name === null);
const prop = (n: Node, key: string) => n.entries.find((e) => e.name === key);
const str = (n: Node, key: string): string | null => {
  const v = prop(n, key)?.value;
  return typeof v === "string" ? v : null;
};
const bool = (n: Node, key: string, fallback: boolean): boolean => {
  const v = prop(n, key)?.value;
  return typeof v === "boolean" ? v : fallback;
};
const err = (n: Node | Entry, message: string): never => {
  throw new Issue(n.line, n.column, message);
};

function textMatch(n: Node, required: true): TextMatch;
function textMatch(n: Node, required: false): TextMatch | null;
function textMatch(n: Node, required: boolean): TextMatch | null {
  const found = n.entries.find((e) => e.name !== null && TEXT_OP_VALUES.includes(e.name as TextOp));
  if (!found) {
    if (required) return err(n, `\`${n.name}\` needs a comparison`);
    return null;
  }
  return {
    op: found.name as TextOp,
    value: String(found.value),
    case_sensitive: bool(n, "case-sensitive", false),
  };
}

function folderRef(n: Node): FolderRef {
  const account = str(n, "account");
  const role = str(n, "role");
  const [path] = args(n);
  if (role) return { type: "role", role: role as FolderRole, account_id: account };
  if (path && typeof path.value === "string") {
    return { type: "path", path: path.value, account_id: account };
  }
  return err(n, `\`${n.name}\` needs a folder`);
}

function condition(n: Node): Condition {
  const [first] = args(n);
  const firstStr = typeof first?.value === "string" ? first.value : "";
  if ((ADDRESS_FIELD_VALUES as readonly string[]).includes(n.name)) {
    return {
      type: "address",
      field: n.name as (typeof ADDRESS_FIELD_VALUES)[number],
      matches: textMatch(n, true),
    };
  }
  switch (n.name) {
    case "all":
    case "any":
      return { type: n.name, conditions: n.children.map(condition) };
    case "not": {
      const kids = n.children.map(condition);
      const [only] = kids;
      return {
        type: "not",
        condition: kids.length === 1 && only ? only : { type: "all", conditions: kids },
      };
    }
    case "header": {
      const header: Condition = { type: "header", name: firstStr, matches: textMatch(n, false) };
      return bool(n, "exists", true) ? header : { type: "not", condition: header };
    }
    case "subject":
    case "body":
      return { type: n.name, matches: textMatch(n, true) };
    case "query":
      return { type: "query", query: firstStr };
    case "attachment": {
      const byName = n.children.find((c) => c.name === "name");
      const byType = n.children.find((c) => c.name === "type");
      return {
        type: "attachment",
        name: byName ? textMatch(byName, true) : null,
        mime: byType ? textMatch(byType, true) : null,
      };
    }
    case "size": {
      const larger = prop(n, "larger");
      const smaller = prop(n, "smaller");
      const v = (larger ?? smaller)?.value;
      if (typeof v !== "number") return err(n, "`size` needs larger=<bytes> or smaller=<bytes>");
      return { type: "size", cmp: larger ? "greater" : "less", bytes: v };
    }
    case "date": {
      const before = str(n, "before");
      const after = str(n, "after");
      const date = before ?? after;
      if (!date || !/^\d{4}-\d{2}-\d{2}$/.test(date)) {
        return err(n, `${JSON.stringify(date ?? "")} is not a date (expected YYYY-MM-DD)`);
      }
      return { type: "date", cmp: before ? "before" : "after", date };
    }
    case "age": {
      const older = prop(n, "older-than");
      const v = (older ?? prop(n, "newer-than"))?.value;
      if (typeof v !== "number")
        return err(n, "`age` needs older-than=<days> or newer-than=<days>");
      return { type: "age", cmp: older ? "greater" : "less", days: v };
    }
    case "flag":
      return {
        type: "flag",
        flag: firstStr as "seen" | "flagged" | "answered" | "draft",
        set: bool(n, "set", true),
      };
    case "tag":
      return { type: "tag", tag: firstStr };
    case "account":
      return { type: "account", account_id: firstStr };
    case "folder":
      return { type: "folder", folder: folderRef(n) };
    case "dkim":
    case "spf":
    case "dmarc":
      return { type: "auth", method: n.name, result: firstStr as "pass" };
    case "list-id":
      return { type: "list-id", matches: textMatch(n, false) };
    default:
      return err(n, `unknown condition \`${n.name}\``);
  }
}

function action(n: Node): RuleAction {
  const [first] = args(n);
  const firstBool = typeof first?.value === "boolean" ? first.value : true;
  const strings = args(n).map((e) => String(e.value));
  switch (n.name) {
    case "move":
    case "copy":
      return { type: n.name, folder: folderRef(n) };
    case "delete":
      return { type: "delete", permanent: bool(n, "permanent", false) };
    case "archive":
    case "stop":
      return { type: n.name };
    case "tag":
    case "untag":
      return { type: n.name, tag: strings[0] ?? "" };
    case "flag":
      return { type: "flag", flagged: firstBool };
    case "mark-read":
      return { type: "mark-read", read: firstBool };
    case "notify":
      return { type: "notify", notify: firstBool };
    case "forward":
      return { type: "forward", to: strings, identity_id: str(n, "identity") };
    case "redirect":
      return { type: "redirect", to: strings };
    case "reply":
      return { type: "reply", template: strings[0] ?? "", identity_id: str(n, "identity") };
    default:
      return err(n, `unknown action \`${n.name}\``);
  }
}

function trigger(value: string, n: Node | Entry): RuleTrigger {
  if (value === "arrival" || value === "on-send" || value === "manual") return { type: value };
  return err(n, `unknown trigger ${JSON.stringify(value)}`);
}

function scope(spec: string, account: string | null, at: Node | Entry): RuleScope {
  const [kind, ...rest] = spec.split(":");
  const value = rest.join(":");
  if ((kind === "account" || kind === "virtual") && value) {
    return { type: "account", account_id: value };
  }
  if (kind === "folder" && value) {
    return { type: "folder", folder: { type: "path", path: value, account_id: account } };
  }
  if (kind === "role" && value) {
    return {
      type: "folder",
      folder: { type: "role", role: value as FolderRole, account_id: account },
    };
  }
  return err(at, `invalid scope ${JSON.stringify(spec)}`);
}

function rule(n: Node): Rule {
  const [nameEntry] = args(n);
  if (!nameEntry || typeof nameEntry.value !== "string" || nameEntry.value.trim() === "") {
    return err(n, '`rule` needs a name: rule "<name>" { … }');
  }
  const name = nameEntry.value.trim();
  const triggers: RuleTrigger[] = [];
  const scopes: RuleScope[] = [];
  const triggerProp = prop(n, "trigger");
  if (triggerProp) triggers.push(trigger(String(triggerProp.value), triggerProp));
  const scopeProp = prop(n, "scope");
  if (scopeProp) scopes.push(scope(String(scopeProp.value), null, scopeProp));
  let conditions: Condition = { type: "all", conditions: [] };
  let actions: RuleAction[] = [];
  for (const child of n.children) {
    const [first] = args(child);
    switch (child.name) {
      case "trigger": {
        const kind = String(first?.value ?? "");
        const every = prop(child, "every")?.value;
        const days = prop(child, "days")?.value;
        if (kind === "schedule") {
          triggers.push({
            type: "schedule",
            interval_minutes: typeof every === "number" ? every : 60,
          });
        } else if (kind === "older-than") {
          triggers.push({ type: "older-than", days: typeof days === "number" ? days : 30 });
        } else {
          triggers.push(trigger(kind, child));
        }
        break;
      }
      case "scope":
        scopes.push(scope(String(first?.value ?? ""), str(child, "account"), child));
        break;
      case "when": {
        const kids = child.children.map(condition);
        conditions =
          first?.value === "any"
            ? { type: "any", conditions: kids }
            : { type: "all", conditions: kids };
        break;
      }
      case "then":
        actions = child.children.map(action);
        break;
      default:
        err(
          child,
          `unknown node \`${child.name}\` in a rule (expected trigger, scope, when or then)`,
        );
    }
  }
  const id = str(n, "id") ?? slug(name);
  return {
    id,
    name,
    enabled: bool(n, "enabled", true),
    triggers,
    scope: scopes,
    conditions,
    actions,
    source: { file: "rule", line: n.line, column: n.column },
  };
}

/** Reads the text form of exactly one rule (like `parse_rule_text`). */
export function ruleFromText(text: string): { rule: Rule | null; errors: ConfigIssue[] } {
  try {
    const nodes = parseKdl(text);
    if (nodes.length !== 1) {
      return {
        rule: null,
        errors: [
          {
            line: 1,
            column: 1,
            message:
              nodes.length === 0
                ? 'expected one `rule "<name>" { … }` node, found none'
                : `expected one \`rule\` node, found ${String(nodes.length)}`,
          },
        ],
      };
    }
    const [node] = nodes as [Node];
    if (node.name !== "rule") err(node, `unknown node \`${node.name}\``);
    return { rule: rule(node), errors: [] };
  } catch (e) {
    if (e instanceof Issue) {
      return { rule: null, errors: [{ line: e.line, column: e.column, message: e.message }] };
    }
    throw e;
  }
}
