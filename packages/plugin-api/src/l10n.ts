/**
 * A small Fluent formatter for the test host's `nail.l10n` (Nail itself
 * formats a plug-in's catalogs in the core with `fluent-bundle`). It
 * covers what plug-in catalogs typically use: messages and terms,
 * multi-line values, `{ $variable }`, string and number literals, message
 * and term references, and select expressions (`{ $count -> [one] … *[other] … }`,
 * plural categories through `Intl.PluralRules` when available).
 * Attributes are read and ignored; no bidi isolation marks are added.
 */
import type { JsonValue } from "./generated";

/** Arguments of a message (`{ $name }`). */
export type L10nArgs = Record<string, string | number | boolean | null | undefined>;

type Pattern = Part[];
type Part =
  | { kind: "text"; text: string }
  | { kind: "var"; name: string }
  | { kind: "literal"; text: string }
  | { kind: "ref"; id: string }
  | { kind: "select"; name: string; variants: Variant[] };
interface Variant {
  key: string;
  isDefault: boolean;
  value: Pattern;
}

const ID = /^-?[a-zA-Z][a-zA-Z0-9_-]*$/;

/** A parsed Fluent catalog (one `.ftl` file). */
export class FluentCatalog {
  private readonly entries = new Map<string, Pattern>();

  /** Parses `source`; entries that do not parse are skipped. */
  constructor(source: string) {
    const lines = source.replace(/\r\n?/g, "\n").split("\n");
    let id: string | null = null;
    let value: string[] = [];
    const finish = () => {
      while (value.length > 0 && value[value.length - 1] === "") value.pop();
      if (id !== null) {
        try {
          this.entries.set(id, parsePattern(value.join("\n")));
        } catch {
          // A malformed entry is left out, like Fluent's junk.
        }
      }
      id = null;
      value = [];
    };
    for (const line of lines) {
      if (line.startsWith("#")) {
        finish();
        continue;
      }
      const head = /^(-?[a-zA-Z][a-zA-Z0-9_-]*)\s*=\s?(.*)$/.exec(line);
      if (head && !/^\s/.test(line)) {
        finish();
        id = head[1] ?? null;
        const first = (head[2] ?? "").trim();
        if (first) value.push(first);
        continue;
      }
      if (id === null) continue;
      if (/^\s+\./.test(line)) {
        // An attribute ends the value; attributes are not used here.
        finish();
        continue;
      }
      if (/^\s+\S/.test(line) || /^(\}|\[|\*\[)/.test(line)) value.push(line.trim());
      else if (line.trim() === "") value.push("");
      else finish();
    }
    finish();
  }

  /** Whether the catalog has message `id` (terms start with `-`). */
  has(id: string): boolean {
    return !id.startsWith("-") && this.entries.has(id);
  }

  /** The ids of the messages. */
  ids(): string[] {
    return [...this.entries.keys()].filter((k) => !k.startsWith("-"));
  }

  /** Formats message `id`, or `undefined` when the catalog does not have it. */
  format(id: string, args: L10nArgs = {}, locale = "en"): string | undefined {
    if (!this.has(id)) return undefined;
    return this.render(this.entries.get(id) ?? [], args, locale, 0);
  }

  private render(pattern: Pattern, args: L10nArgs, locale: string, depth: number): string {
    if (depth > 10) return "";
    let out = "";
    for (const part of pattern) {
      switch (part.kind) {
        case "text":
        case "literal":
          out += part.text;
          break;
        case "var": {
          const v = args[part.name];
          out += v === undefined || v === null ? `{$${part.name}}` : String(v);
          break;
        }
        case "ref": {
          const target = this.entries.get(part.id);
          out += target ? this.render(target, args, locale, depth + 1) : `{${part.id}}`;
          break;
        }
        case "select": {
          const variant = pick(part.variants, args[part.name], locale);
          out += variant ? this.render(variant.value, args, locale, depth + 1) : "";
          break;
        }
      }
    }
    return out;
  }
}

function pick(variants: Variant[], value: unknown, locale: string): Variant | undefined {
  const text = value === undefined || value === null ? undefined : String(value);
  const exact = variants.find((v) => v.key === text);
  if (exact) return exact;
  if (typeof value === "number") {
    const Plural = (globalThis as { Intl?: typeof Intl }).Intl?.PluralRules;
    if (Plural) {
      const category = new Plural(locale).select(value);
      const plural = variants.find((v) => v.key === category);
      if (plural) return plural;
    }
  }
  return variants.find((v) => v.isDefault) ?? variants[variants.length - 1];
}

/** Parses a value pattern (text with placeables). */
function parsePattern(src: string): Pattern {
  const [pattern, end] = readPattern(src, 0, false);
  if (end !== src.length) throw new SyntaxError("unexpected text");
  return pattern;
}

/**
 * Reads text and placeables from `i`. Inside a select variant it stops at
 * a line that starts the next variant (`[`, `*[`) or closes the
 * expression (`}`).
 */
function readPattern(src: string, start: number, inVariant: boolean): [Pattern, number] {
  const parts: Pattern = [];
  let text = "";
  let i = start;
  while (i < src.length) {
    const c = src[i];
    if (inVariant && c === "\n") {
      const rest = src.slice(i + 1).replace(/^\s*/, "");
      if (rest.startsWith("[") || rest.startsWith("*[") || rest.startsWith("}")) break;
    }
    if (c === "{") {
      if (text) parts.push({ kind: "text", text });
      text = "";
      const [part, next] = readPlaceable(src, i + 1);
      parts.push(part);
      i = next;
      continue;
    }
    if (c === "}") {
      if (inVariant) break;
      throw new SyntaxError("unbalanced }");
    }
    text += c;
    i += 1;
  }
  if (text) parts.push({ kind: "text", text: inVariant ? text.replace(/\s+$/, "") : text });
  return [parts, i];
}

function skipSpace(src: string, i: number): number {
  while (i < src.length && /\s/.test(src[i] ?? "")) i += 1;
  return i;
}

/** Reads a placeable after its `{`; returns the part and the index after `}`. */
function readPlaceable(src: string, start: number): [Part, number] {
  let i = skipSpace(src, start);
  let part: Part;
  if (src[i] === '"') {
    let text = "";
    i += 1;
    while (i < src.length && src[i] !== '"') {
      if (src[i] === "\\" && i + 1 < src.length) {
        const esc = src[i + 1];
        text += esc === "u" ? String.fromCharCode(parseInt(src.slice(i + 2, i + 6), 16)) : esc;
        i += esc === "u" ? 6 : 2;
      } else {
        text += src[i];
        i += 1;
      }
    }
    i += 1;
    part = { kind: "literal", text };
  } else if (src[i] === "$") {
    const m = /^\$([a-zA-Z][a-zA-Z0-9_-]*)/.exec(src.slice(i));
    if (!m) throw new SyntaxError("bad variable");
    i += m[0].length;
    const name = m[1] ?? "";
    const after = skipSpace(src, i);
    if (src.startsWith("->", after)) {
      const [variants, next] = readVariants(src, after + 2);
      return [{ kind: "select", name, variants }, next];
    }
    part = { kind: "var", name };
  } else {
    const m = /^(-?[a-zA-Z][a-zA-Z0-9_-]*|-?\d+(\.\d+)?)/.exec(src.slice(i));
    if (!m) throw new SyntaxError("bad placeable");
    i += m[0].length;
    const token = m[0];
    part = ID.test(token) ? { kind: "ref", id: token } : { kind: "literal", text: token };
  }
  i = skipSpace(src, i);
  if (src[i] !== "}") throw new SyntaxError("expected }");
  return [part, i + 1];
}

/** Reads `[key] pattern` variants up to the closing `}`. */
function readVariants(src: string, start: number): [Variant[], number] {
  const variants: Variant[] = [];
  let i = start;
  for (;;) {
    i = skipSpace(src, i);
    if (i >= src.length) throw new SyntaxError("unterminated select");
    if (src[i] === "}") break;
    let isDefault = false;
    if (src[i] === "*") {
      isDefault = true;
      i += 1;
    }
    if (src[i] !== "[") throw new SyntaxError("expected [");
    const close = src.indexOf("]", i);
    if (close < 0) throw new SyntaxError("expected ]");
    const key = src.slice(i + 1, close).trim();
    const [value, next] = readPattern(src, skipInline(src, close + 1), true);
    variants.push({ key, isDefault, value });
    i = next;
  }
  if (!variants.some((v) => v.isDefault)) throw new SyntaxError("no default variant");
  return [variants, i + 1];
}

function skipInline(src: string, i: number): number {
  while (i < src.length && (src[i] === " " || src[i] === "\t")) i += 1;
  return i;
}

/** What {@link createLocalizer} returns: the shape of `nail.l10n`. */
export interface Localizer {
  /** The language in use (`en`, `de`, `pt-BR`). */
  readonly locale: string;
  /** Message `id` formatted with `args`: the locale's, else English, else the id. */
  t(id: string, args?: L10nArgs): string;
  /** Whether the locale's catalog or the English one has message `id`. */
  has(id: string): boolean;
}

/**
 * A localizer over a plug-in's catalogs (language tag → `.ftl` text),
 * resolved like Nail does: the exact tag, then its language (`pt` for
 * `pt-BR`), then English per message, then the id itself.
 */
export function createLocalizer(locale: string, catalogs: Record<string, string>): Localizer {
  const parsed = new Map<string, FluentCatalog>();
  for (const [tag, source] of Object.entries(catalogs)) parsed.set(tag, new FluentCatalog(source));
  const base = locale.split("-")[0] ?? locale;
  const chain = [locale, base, "en"]
    .filter((tag, i, all) => all.indexOf(tag) === i)
    .map((tag) => parsed.get(tag))
    .filter((c): c is FluentCatalog => c !== undefined);
  return {
    locale,
    t(id, args = {}) {
      for (const catalog of chain) {
        const text = catalog.format(id, args, locale);
        if (text !== undefined) return text;
      }
      return id;
    },
    has: (id) => chain.some((c) => c.has(id)),
  };
}

/** Fluent arguments from JSON values (objects and arrays become JSON text). */
export function toL10nArgs(values: Record<string, JsonValue>): L10nArgs {
  const out: L10nArgs = {};
  for (const [k, v] of Object.entries(values)) {
    out[k] = v === null || typeof v !== "object" ? v : JSON.stringify(v);
  }
  return out;
}
