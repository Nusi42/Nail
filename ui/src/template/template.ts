/**
 * The row / header template language (design.md, "Layout file" → Templates).
 *
 *   template := ( text | "{{" | "}}" | "{" expr "}" )*
 *   expr     := path ( "|" pipe )*
 *   path     := ident ( "." ident )*
 *   pipe     := name [ "(" arg ( "," arg )* ")" ]
 *   arg      := integer | "quoted string"
 *
 * P2 (WP202): `then("text")` appends `text` to a field that is not empty,
 * inside the field's own element (`{latest_from | names(1) | then(":")}`
 * writes "Grace:" but nothing for a message without a sender).
 *
 * Templates are data, not code: they are parsed by hand, fields are looked up
 * as own properties only, and pipes come from a fixed table. Unknown pipes,
 * wrong arguments and syntax errors are rejected at compile time.
 */
import { formatRelative, formatShortDate } from "../i18n/format";
import { chars } from "../text/chars";
import { nonEmpty } from "../text/empty";

/** A value flowing through pipes. */
export type TemplateValue =
  string | number | boolean | null | TemplateValue[] | { [key: string]: TemplateValue };

/** One name of a `names(N)` segment (P2, ADR 0148). */
export interface NameItem {
  /** The display name (the context's `me` text for the user). */
  text: string;
  /** The user's own address. */
  me: boolean;
  /** The person sent an unread message (shown bold). */
  unread: boolean;
}

/** A rendered piece of a template line. */
export type Segment =
  | { kind: "text"; text: string }
  | {
      kind: "field";
      field: string;
      text: string;
      strong: boolean;
      /** `then("…")`: text written after a non-empty value. */
      suffix?: string;
    }
  | { kind: "chips"; field: string; items: string[] }
  /**
   * `{participants | names(N)}`: at most N names; with more people the
   * first N - 1 and the newest, `skipped` = how many are left out between
   * them.
   */
  | {
      kind: "names";
      field: string;
      items: NameItem[];
      skipped: number;
      /** `then("…")`: text written after the names when there are any. */
      suffix?: string;
    };

/** Formatting context for pipes. */
export interface TemplateContext {
  /** BCP 47 locale for `Intl`. */
  locale: string;
  /** "Now" in ms since the epoch (for `relative`). */
  now: number;
  /** Whether the current item is unread (for `bold-if-unread`). */
  unread: boolean;
  /** How `names` writes the user ("me"; P2). Absent: the address's name. */
  me?: string;
}

interface Pipe {
  name: string;
  args: (string | number)[];
}

interface FieldNode {
  kind: "field";
  path: string[];
  pipes: Pipe[];
}

type Node = { kind: "text"; text: string } | FieldNode;

/** A parsed template, ready to render. */
export interface CompiledTemplate {
  source: string;
  nodes: Node[];
}

export class TemplateError extends Error {
  readonly position: number;

  constructor(message: string, position: number) {
    super(`${message} at column ${String(position + 1)}`);
    this.name = "TemplateError";
    this.position = position;
  }
}

interface PipeSpec {
  /** Allowed argument shapes. */
  args: "none" | "positive-int" | "string";
}

/** The fixed pipe table. */
export const PIPES: Readonly<Record<string, PipeSpec>> = {
  relative: { args: "none" },
  "short-date": { args: "none" },
  truncate: { args: "positive-int" },
  "bold-if-unread": { args: "none" },
  chips: { args: "none" },
  // P2 (ADR 0148): the conversation row's people and its count pill.
  names: { args: "positive-int" },
  "if-many": { args: "none" },
  // P2 WP202: a separator that follows a non-empty value ("Sender:").
  then: { args: "string" },
};

const IDENT_START = /[A-Za-z_]/;
const IDENT_CHAR = /[A-Za-z0-9_-]/;

class Parser {
  private pos = 0;
  private readonly src: string;

  constructor(src: string) {
    this.src = src;
  }

  parse(): Node[] {
    const nodes: Node[] = [];
    let text = "";
    const flush = () => {
      if (text) nodes.push({ kind: "text", text });
      text = "";
    };
    while (this.pos < this.src.length) {
      const c = this.src.charAt(this.pos);
      const next = this.src[this.pos + 1];
      if (c === "{" && next === "{") {
        text += "{";
        this.pos += 2;
      } else if (c === "}" && next === "}") {
        text += "}";
        this.pos += 2;
      } else if (c === "{") {
        flush();
        this.pos++;
        nodes.push(this.expr());
      } else if (c === "}") {
        throw new TemplateError('unmatched "}" (write "}}" for a literal brace)', this.pos);
      } else {
        text += c;
        this.pos++;
      }
    }
    flush();
    return nodes;
  }

  private skipSpace(): void {
    while (this.src[this.pos] === " " || this.src[this.pos] === "\t") this.pos++;
  }

  private ident(what: string): string {
    this.skipSpace();
    const start = this.pos;
    if (!IDENT_START.test(this.src[this.pos] ?? "")) {
      throw new TemplateError(`expected ${what}`, this.pos);
    }
    while (IDENT_CHAR.test(this.src[this.pos] ?? "")) this.pos++;
    return this.src.slice(start, this.pos);
  }

  private expect(char: string): void {
    this.skipSpace();
    if (this.src[this.pos] !== char) {
      throw new TemplateError(`expected "${char}"`, this.pos);
    }
    this.pos++;
  }

  private expr(): FieldNode {
    const path = [this.ident("a field name")];
    while (this.src[this.pos] === ".") {
      this.pos++;
      path.push(this.ident('a field name after "."'));
    }
    const pipes: Pipe[] = [];
    for (;;) {
      this.skipSpace();
      const c = this.src[this.pos];
      if (c === "}") {
        this.pos++;
        return { kind: "field", path, pipes };
      }
      if (c !== "|") {
        throw new TemplateError(c === undefined ? 'unclosed "{"' : `unexpected "${c}"`, this.pos);
      }
      this.pos++;
      pipes.push(this.pipe());
    }
  }

  private pipe(): Pipe {
    this.skipSpace();
    const start = this.pos;
    const name = this.ident("a pipe name");
    const spec = PIPES[name];
    if (!spec || !Object.hasOwn(PIPES, name)) {
      throw new TemplateError(`unknown pipe "${name}"`, start);
    }
    const args: (string | number)[] = [];
    this.skipSpace();
    if (this.src[this.pos] === "(") {
      this.pos++;
      for (;;) {
        args.push(this.arg());
        this.skipSpace();
        if (this.src[this.pos] === ",") {
          this.pos++;
          continue;
        }
        this.expect(")");
        break;
      }
    }
    if (spec.args === "none" && args.length > 0) {
      throw new TemplateError(`pipe "${name}" takes no arguments`, start);
    }
    if (
      spec.args === "positive-int" &&
      (args.length !== 1 ||
        typeof args[0] !== "number" ||
        !Number.isInteger(args[0]) ||
        args[0] < 1)
    ) {
      throw new TemplateError(`pipe "${name}" takes one positive whole number`, start);
    }
    if (spec.args === "string" && (args.length !== 1 || typeof args[0] !== "string")) {
      throw new TemplateError(`pipe "${name}" takes one quoted text`, start);
    }
    return { name, args };
  }

  private arg(): string | number {
    this.skipSpace();
    const start = this.pos;
    if (this.src[this.pos] === '"') {
      this.pos++;
      let value = "";
      while (this.pos < this.src.length && this.src[this.pos] !== '"') {
        value += this.src.charAt(this.pos);
        this.pos++;
      }
      if (this.src[this.pos] !== '"') throw new TemplateError("unclosed string", start);
      this.pos++;
      return value;
    }
    while (/[0-9-]/.test(this.src[this.pos] ?? "")) this.pos++;
    const text = this.src.slice(start, this.pos);
    if (!/^-?[0-9]+$/.test(text)) {
      throw new TemplateError("expected a number or a quoted string", start);
    }
    return Number(text);
  }
}

/** Parses a template; throws `TemplateError` on invalid input. */
export function compileTemplate(source: string): CompiledTemplate {
  return { source, nodes: new Parser(source).parse() };
}

function isAddress(value: unknown): value is { name: string | null; email: string } {
  return (
    value !== null &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    typeof (value as { email?: unknown }).email === "string" &&
    "name" in value
  );
}

/** Looks up `path` on own properties only (no prototype access). */
export function lookup(root: unknown, path: readonly string[]): unknown {
  let current: unknown = root;
  for (const [index, key] of path.entries()) {
    if (current === null || typeof current !== "object" || Array.isArray(current)) {
      return null;
    }
    // An address without a display name shows its email instead.
    if (key === "name" && index === path.length - 1 && isAddress(current) && !current.name) {
      return current.email;
    }
    if (!Object.hasOwn(current, key)) {
      return null;
    }
    current = (current as Record<string, unknown>)[key];
  }
  return current;
}

/** Plain-text rendering of a value. */
export function display(value: unknown): string {
  if (value === null || value === undefined) return "";
  if (Array.isArray(value)) return value.map(display).filter(Boolean).join(", ");
  if (isAddress(value)) return nonEmpty(value.name) ?? value.email;
  switch (typeof value) {
    case "string":
      return value;
    case "number":
    case "boolean":
    case "bigint":
      return String(value);
    default:
      return "";
  }
}

function toDate(value: unknown): Date | null {
  if (typeof value !== "string" && typeof value !== "number") return null;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
}

// The `relative` and `date` pipes format in the context's locale (the UI
// locale, i18n/format.ts).
export { formatRelative, formatShortDate };

/** Part suffix for a field path: `from.name` → `from`, `has_attachments` → `has-attachments`. */
export function fieldPart(path: readonly string[]): string {
  return (path[0] ?? "field").toLowerCase().replaceAll("_", "-");
}

/**
 * The names of a list of people (`ThreadParticipant`s or plain addresses):
 * at most `max`, the first `max - 1` and the newest when there are more
 * (P2, ADR 0148).
 */
export function nameItems(
  value: unknown,
  max: number,
  me: string | undefined,
): { items: NameItem[]; skipped: number } {
  const people = (Array.isArray(value) ? value : [value]).filter(isAddress);
  const item = (person: { name: string | null; email: string }): NameItem => {
    const flags = person as { me?: unknown; unread?: unknown };
    const mine = flags.me === true;
    return {
      text: mine && me !== undefined ? me : display(person),
      me: mine,
      unread: flags.unread === true,
    };
  };
  if (people.length <= max) return { items: people.map(item), skipped: 0 };
  const head = people.slice(0, Math.max(0, max - 1)).map(item);
  const last = people[people.length - 1];
  return {
    items: last ? [...head, item(last)] : head,
    skipped: people.length - head.length - 1,
  };
}

/** Renders a compiled template against a data object. */
export function renderTemplate(
  template: CompiledTemplate,
  data: unknown,
  ctx: TemplateContext,
): Segment[] {
  const out: Segment[] = [];
  for (const node of template.nodes) {
    if (node.kind === "text") {
      out.push({ kind: "text", text: node.text });
      continue;
    }
    let value = lookup(data, node.path);
    let strong = false;
    let chips: string[] | null = null;
    let names: { items: NameItem[]; skipped: number } | null = null;
    let suffix: string | null = null;
    for (const pipe of node.pipes) {
      switch (pipe.name) {
        case "relative": {
          const date = toDate(value);
          value = date ? formatRelative(date, ctx.now, ctx.locale) : display(value);
          break;
        }
        case "short-date": {
          const date = toDate(value);
          value = date ? formatShortDate(date, ctx.now, ctx.locale) : display(value);
          break;
        }
        case "truncate": {
          const max = pipe.args[0] as number;
          const text = display(value);
          const points = chars(text);
          value = points.length > max ? `${points.slice(0, max).join("").trimEnd()}…` : text;
          break;
        }
        case "bold-if-unread":
          strong = strong || ctx.unread;
          break;
        case "chips":
          chips = (Array.isArray(value) ? value.map(display) : display(value).split(/\s+/)).filter(
            Boolean,
          );
          break;
        case "names":
          names = nameItems(value, pipe.args[0] as number, ctx.me);
          break;
        case "if-many":
          value = typeof value === "number" && value > 1 ? value : "";
          break;
        case "then":
          suffix = pipe.args[0] as string;
          break;
      }
    }
    const field = fieldPart(node.path);
    if (names) {
      out.push({
        kind: "names",
        field,
        ...names,
        ...(suffix !== null && names.items.length > 0 ? { suffix } : {}),
      });
    } else if (chips) {
      out.push({ kind: "chips", field, items: chips });
    } else {
      const text = display(value);
      out.push({
        kind: "field",
        field,
        text,
        strong,
        ...(suffix !== null && text !== "" ? { suffix } : {}),
      });
    }
  }
  return out;
}
