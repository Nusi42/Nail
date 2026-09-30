/**
 * The row / header template language (design.md, "Layout file" → Templates).
 *
 *   template := ( text | "{{" | "}}" | "{" expr "}" )*
 *   expr     := path ( "|" pipe )*
 *   path     := ident ( "." ident )*
 *   pipe     := name [ "(" arg ( "," arg )* ")" ]
 *   arg      := integer | "quoted string"
 *
 * Templates are data, not code: they are parsed by hand, fields are looked up
 * as own properties only, and pipes come from a fixed table. Unknown pipes,
 * wrong arguments and syntax errors are rejected at compile time.
 */
import { formatRelative, formatShortDate } from "../i18n/format";

/** A value flowing through pipes. */
export type TemplateValue =
  string | number | boolean | null | TemplateValue[] | { [key: string]: TemplateValue };

/** A rendered piece of a template line. */
export type Segment =
  | { kind: "text"; text: string }
  | { kind: "field"; field: string; text: string; strong: boolean }
  | { kind: "chips"; field: string; items: string[] };

/** Formatting context for pipes. */
export interface TemplateContext {
  /** BCP 47 locale for `Intl`. */
  locale: string;
  /** "Now" in ms since the epoch (for `relative`). */
  now: number;
  /** Whether the current item is unread (for `bold-if-unread`). */
  unread: boolean;
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
    super(`${message} at column ${position + 1}`);
    this.name = "TemplateError";
    this.position = position;
  }
}

interface PipeSpec {
  /** Allowed argument shapes. */
  args: "none" | "positive-int";
}

/** The fixed pipe table. */
export const PIPES: Readonly<Record<string, PipeSpec>> = {
  relative: { args: "none" },
  "short-date": { args: "none" },
  truncate: { args: "positive-int" },
  "bold-if-unread": { args: "none" },
  chips: { args: "none" },
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
      const c = this.src[this.pos] as string;
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
    return { name, args };
  }

  private arg(): string | number {
    this.skipSpace();
    const start = this.pos;
    if (this.src[this.pos] === '"') {
      this.pos++;
      let value = "";
      while (this.pos < this.src.length && this.src[this.pos] !== '"') {
        value += this.src[this.pos];
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
  if (isAddress(value)) return value.name || value.email;
  if (typeof value === "object") return "";
  return String(value);
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
          const chars = [...text];
          value = chars.length > max ? `${chars.slice(0, max).join("").trimEnd()}…` : text;
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
      }
    }
    const field = fieldPart(node.path);
    if (chips) {
      out.push({ kind: "chips", field, items: chips });
    } else {
      out.push({ kind: "field", field, text: display(value), strong });
    }
  }
  return out;
}
