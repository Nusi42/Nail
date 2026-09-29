/**
 * A small implementation of the search query language (design.md, "Threading
 * and search") for the mock core, so the search bar can be developed and
 * tested without the Rust parser. `nail-search` (WP13) is authoritative; this
 * mirrors its error kinds and UTF-16 positions closely enough for the UI.
 *
 *   query   := or
 *   or      := and ( "OR" and )*
 *   and     := unary ( ["AND"] unary )*
 *   unary   := ( "NOT" | "-" ) unary | primary
 *   primary := "(" or ")" | field ":" value | "size" (">"|"<") number | word | "quoted"
 */
import type { SearchError, SearchErrorKind } from "./generated";

/** Fields of the query language, in the order the search bar suggests them. */
export const SEARCH_FIELDS = [
  "from",
  "to",
  "subject",
  "has",
  "is",
  "tag",
  "account",
  "folder",
  "before",
  "after",
] as const;

export type SearchField = (typeof SEARCH_FIELDS)[number];

const IS_VALUES = ["unread", "read", "flagged", "unflagged", "answered", "draft"] as const;
const HAS_VALUES = ["attachment", "attachments"] as const;

export type QueryNode =
  | { type: "and"; items: QueryNode[] }
  | { type: "or"; items: QueryNode[] }
  | { type: "not"; item: QueryNode }
  | { type: "text"; value: string }
  | { type: "field"; field: SearchField; value: string }
  | { type: "size"; op: ">" | "<"; bytes: number };

type Token =
  | { kind: "lparen" | "rparen" | "and" | "or" | "not"; start: number; end: number }
  | { kind: "term"; node: QueryNode; start: number; end: number };

class QueryError extends Error {
  readonly error: SearchError;
  constructor(kind: SearchErrorKind, message: string, start: number, end: number) {
    super(message);
    this.error = { kind, message, start, end };
  }
}

const STOP = /[\s()"]/;

function readQuoted(text: string, at: number): { value: string; end: number } {
  const close = text.indexOf('"', at + 1);
  if (close < 0) {
    throw new QueryError("unterminated-quote", "missing closing quote", at, text.length);
  }
  return { value: text.slice(at + 1, close), end: close + 1 };
}

function readWord(text: string, at: number): number {
  let i = at;
  while (i < text.length && !STOP.test(text[i] as string)) i++;
  return i;
}

function parseSize(raw: string): number | null {
  const m = /^(\d+(?:\.\d+)?)([kmg]b?|b)?$/i.exec(raw);
  if (!m) return null;
  const unit = (m[2] ?? "").toLowerCase().replace(/b$/, "");
  const factor = unit === "k" ? 1024 : unit === "m" ? 1024 ** 2 : unit === "g" ? 1024 ** 3 : 1;
  return Number(m[1]) * factor;
}

function checkValue(field: SearchField, value: string, start: number, end: number): void {
  const bad = (what: string) => {
    throw new QueryError("invalid-value", what, start, end);
  };
  if (field === "is" && !(IS_VALUES as readonly string[]).includes(value.toLowerCase())) {
    bad(`is: expects one of ${IS_VALUES.join(", ")}`);
  }
  if (field === "has" && !(HAS_VALUES as readonly string[]).includes(value.toLowerCase())) {
    bad("has: expects attachment");
  }
  if ((field === "before" || field === "after") && !/^\d{4}-\d{2}-\d{2}$/.test(value)) {
    bad(`${field}: expects a date like 2026-09-30`);
  }
  if ((field === "before" || field === "after") && Number.isNaN(Date.parse(value))) {
    bad(`${field}: expects a valid date`);
  }
}

function tokenize(text: string): Token[] {
  const tokens: Token[] = [];
  let i = 0;
  while (i < text.length) {
    const c = text[i] as string;
    if (/\s/.test(c)) {
      i++;
    } else if (c === "(" || c === ")") {
      tokens.push({ kind: c === "(" ? "lparen" : "rparen", start: i, end: i + 1 });
      i++;
    } else if (c === '"') {
      const { value, end } = readQuoted(text, i);
      tokens.push({ kind: "term", node: { type: "text", value }, start: i, end });
      i = end;
    } else if (c === "-" && i + 1 < text.length && !STOP.test(text[i + 1] as string)) {
      tokens.push({ kind: "not", start: i, end: i + 1 });
      i++;
    } else {
      const wordEnd = readWord(text, i);
      const word = text.slice(i, wordEnd);
      const colon = word.indexOf(":");
      const size = /^size([<>])(.*)$/i.exec(word);
      if (size) {
        const bytes = parseSize(size[2] ?? "");
        if (size[2] === "" && wordEnd === text.length) {
          throw new QueryError("unexpected-end", "size needs a number", wordEnd, wordEnd);
        }
        if (bytes === null) {
          throw new QueryError("invalid-value", "size expects a number", i + 5, wordEnd);
        }
        const op = size[1] as ">" | "<";
        tokens.push({ kind: "term", node: { type: "size", op, bytes }, start: i, end: wordEnd });
        i = wordEnd;
      } else if (colon > 0 && /^[a-z]+$/i.test(word.slice(0, colon))) {
        const name = word.slice(0, colon).toLowerCase();
        if (!(SEARCH_FIELDS as readonly string[]).includes(name)) {
          throw new QueryError("unknown-field", `unknown field ${name}`, i, i + colon);
        }
        const field = name as SearchField;
        const valueStart = i + colon + 1;
        let value: string;
        let end: number;
        if (text[valueStart] === '"') {
          ({ value, end } = readQuoted(text, valueStart));
        } else {
          end = readWord(text, valueStart);
          value = text.slice(valueStart, end);
        }
        if (value === "") {
          if (end === text.length) {
            throw new QueryError("unexpected-end", `${field}: needs a value`, end, end);
          }
          throw new QueryError("invalid-value", `${field}: needs a value`, valueStart, valueStart);
        }
        checkValue(field, value, valueStart, end);
        tokens.push({ kind: "term", node: { type: "field", field, value }, start: i, end });
        i = end;
      } else if (word === "AND" || word === "OR" || word === "NOT") {
        const kind = word === "AND" ? "and" : word === "OR" ? "or" : "not";
        tokens.push({ kind, start: i, end: wordEnd });
        i = wordEnd;
      } else {
        tokens.push({ kind: "term", node: { type: "text", value: word }, start: i, end: wordEnd });
        i = wordEnd;
      }
    }
  }
  return tokens;
}

class Parser {
  private pos = 0;
  constructor(
    private readonly tokens: Token[],
    private readonly length: number,
  ) {}

  private peek(): Token | undefined {
    return this.tokens[this.pos];
  }

  private endError(what: string): QueryError {
    return new QueryError("unexpected-end", what, this.length, this.length);
  }

  parse(): QueryNode {
    const node = this.or();
    const rest = this.peek();
    if (rest) {
      throw rest.kind === "rparen"
        ? new QueryError("unbalanced-parenthesis", "no matching (", rest.start, rest.end)
        : new QueryError("unexpected-token", "unexpected input", rest.start, rest.end);
    }
    return node;
  }

  private or(): QueryNode {
    const items = [this.and()];
    while (this.peek()?.kind === "or") {
      this.pos++;
      if (!this.peek()) throw this.endError("OR needs a right-hand side");
      items.push(this.and());
    }
    return items.length === 1 ? (items[0] as QueryNode) : { type: "or", items };
  }

  private and(): QueryNode {
    const items = [this.unary()];
    for (;;) {
      const next = this.peek();
      if (!next || next.kind === "or" || next.kind === "rparen") break;
      if (next.kind === "and") {
        this.pos++;
        if (!this.peek()) throw this.endError("AND needs a right-hand side");
      }
      items.push(this.unary());
    }
    return items.length === 1 ? (items[0] as QueryNode) : { type: "and", items };
  }

  private unary(): QueryNode {
    const next = this.peek();
    if (next?.kind === "not") {
      this.pos++;
      if (!this.peek()) throw this.endError("NOT needs something to negate");
      return { type: "not", item: this.unary() };
    }
    return this.primary();
  }

  private primary(): QueryNode {
    const next = this.peek();
    if (!next) throw this.endError("the query ends too early");
    if (next.kind === "lparen") {
      this.pos++;
      const inner = this.peek();
      if (!inner) {
        throw new QueryError("unbalanced-parenthesis", "( is not closed", next.start, next.end);
      }
      if (inner.kind === "rparen") {
        throw new QueryError("unexpected-token", "empty parentheses", inner.start, inner.end);
      }
      const node = this.or();
      if (this.peek()?.kind !== "rparen") {
        throw new QueryError("unbalanced-parenthesis", "( is not closed", next.start, next.end);
      }
      this.pos++;
      return node;
    }
    if (next.kind === "term") {
      this.pos++;
      return next.node;
    }
    if (next.kind === "rparen") {
      throw new QueryError("unbalanced-parenthesis", "no matching (", next.start, next.end);
    }
    throw new QueryError(
      "unexpected-token",
      `${next.kind.toUpperCase()} cannot start a term`,
      next.start,
      next.end,
    );
  }
}

/** Parses a query; throws nothing, returns the error instead. */
export function parseQuery(text: string): { node: QueryNode | null; error: SearchError | null } {
  try {
    const tokens = tokenize(text);
    if (tokens.length === 0) return { node: null, error: null };
    return { node: new Parser(tokens, text.length).parse(), error: null };
  } catch (error) {
    if (error instanceof QueryError) return { node: null, error: error.error };
    throw error;
  }
}

/** What a query can be matched against. */
export interface Searchable {
  subject: string;
  from: string;
  to: string;
  text: string;
  tags: string[];
  account: string;
  accountName: string;
  folder: string;
  date: number;
  size: number;
  unread: boolean;
  flagged: boolean;
  answered: boolean;
  draft: boolean;
  hasAttachments: boolean;
}

const includes = (hay: string, needle: string) => hay.toLowerCase().includes(needle.toLowerCase());

/** Evaluates a parsed query against one message. */
export function matches(node: QueryNode, m: Searchable): boolean {
  switch (node.type) {
    case "and":
      return node.items.every((item) => matches(item, m));
    case "or":
      return node.items.some((item) => matches(item, m));
    case "not":
      return !matches(node.item, m);
    case "text":
      return [m.subject, m.from, m.to, m.text].some((field) => includes(field, node.value));
    case "size":
      return node.op === ">" ? m.size > node.bytes : m.size < node.bytes;
    case "field": {
      const v = node.value;
      switch (node.field) {
        case "from":
          return includes(m.from, v);
        case "to":
          return includes(m.to, v);
        case "subject":
          return includes(m.subject, v);
        case "tag":
          return m.tags.some((t) => t.toLowerCase() === v.toLowerCase());
        case "account":
          return m.account === v || includes(m.accountName, v);
        case "folder":
          return includes(m.folder, v);
        case "has":
          return m.hasAttachments;
        case "before":
          return m.date < Date.parse(v);
        case "after":
          return m.date >= Date.parse(v);
        case "is":
          switch (v.toLowerCase()) {
            case "unread":
              return m.unread;
            case "read":
              return !m.unread;
            case "flagged":
              return m.flagged;
            case "unflagged":
              return !m.flagged;
            case "answered":
              return m.answered;
            default:
              return m.draft;
          }
      }
    }
  }
}
