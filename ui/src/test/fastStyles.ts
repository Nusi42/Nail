/**
 * Fast computed styles for the jsdom suites.
 *
 * Why: the compiled theme puts about 2,400 rules into the test document,
 * nearly all of them keyed on `[data-part="…"]`. jsdom's `getComputedStyle`
 * runs every rule's selector against the element (its own pre-filter only
 * knows ids, classes and tag names), and does that again for each ancestor
 * of an inherited property (`visibility`, `pointer-events`). Testing
 * Library's role queries (accessible names, `isInaccessible`), user-event's
 * pointer-events check and axe call it for nearly every element after every
 * DOM change: one element costs ~30 ms, a role query over a dialog seconds.
 * Under machine load the larger widget suites then ran into the 20 s test
 * timeout.
 *
 * What: a pre-filter in front of jsdom's selector check. A rule whose
 * selector requires `[data-part="x"]` on its subject (the rightmost
 * compound) — in every branch of its selector list — cannot match an element
 * whose `data-part` is not one of those values, so it is rejected without
 * running the selector engine. Every other rule goes through jsdom
 * unchanged, so the computed style is the same as without the filter
 * (`fastStyles.test.tsx` checks that on rendered widgets).
 *
 * The filter reaches into jsdom internals (`Document#_getDOMSelector`); when
 * a jsdom update changes them, `installFastStyles` installs nothing and the
 * self-test fails, instead of the suites silently getting slow again.
 */
import { createRequire } from "node:module";

interface ElementImpl {
  getAttributeNS: (namespace: string | null, name: string) => string | null;
}

interface CheckResult {
  match: boolean;
  ast?: unknown;
  pseudoElement?: unknown;
}

type Check = (selector: string, node: ElementImpl, options?: unknown) => CheckResult;

interface DomSelector {
  check: Check;
}

interface DocumentImpl {
  _getDOMSelector: () => DomSelector;
  _clearStyleCache: () => void;
}

const REJECTED: CheckResult = { match: false, ast: null, pseudoElement: null };

/** Splits `text` at `separator` characters outside brackets, parentheses and strings. */
function splitTopLevel(text: string, isSeparator: (char: string) => boolean): string[] {
  const parts: string[] = [];
  let depth = 0;
  let quote: string | null = null;
  let start = 0;
  for (let i = 0; i < text.length; i++) {
    const char = text.charAt(i);
    if (quote !== null) {
      if (char === "\\") i++;
      else if (char === quote) quote = null;
    } else if (char === '"' || char === "'") {
      quote = char;
    } else if (char === "\\") {
      i++;
    } else if (char === "[" || char === "(") {
      depth++;
    } else if (char === "]" || char === ")") {
      depth--;
    } else if (depth === 0 && isSeparator(char)) {
      parts.push(text.slice(start, i));
      start = i + 1;
    }
  }
  parts.push(text.slice(start));
  return parts;
}

const COMBINATOR = /[\s>+~]/;
const PART_ATTRIBUTE = /^\[\s*data-part\s*=\s*(?:"([^"\\]*)"|'([^'\\]*)'|([\w-][\w.-]*))\s*\]$/;

/** The `[data-part="…"]` value a compound selector requires at its top level, if any. */
function requiredPart(compound: string): string | null {
  let parens = 0;
  for (let i = 0; i < compound.length; i++) {
    const char = compound.charAt(i);
    if (char === "\\") return null;
    if (char === "(") parens++;
    else if (char === ")") parens--;
    else if (char === "[" && parens === 0) {
      const end = compound.indexOf("]", i);
      if (end < 0) return null;
      const match = PART_ATTRIBUTE.exec(compound.slice(i, end + 1));
      if (match) return match[1] ?? match[2] ?? match[3] ?? null;
      i = end;
    }
  }
  return null;
}

/**
 * The `data-part` values one of which an element must have to match
 * `selector`, or `null` when the selector does not require one (some branch
 * of the list has no `[data-part="…"]` on its subject).
 */
export function subjectParts(selector: string): string[] | null {
  const parts: string[] = [];
  for (const branch of splitTopLevel(selector, (char) => char === ",")) {
    const compounds = splitTopLevel(branch.trim(), (char) => COMBINATOR.test(char));
    const subject = compounds.at(-1) ?? "";
    const part = requiredPart(subject);
    if (part === null) return null;
    parts.push(part);
  }
  return parts.length > 0 ? parts : null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return (typeof value === "object" || typeof value === "function") && value !== null;
}

function documentImpl(doc: Document): DocumentImpl | null {
  try {
    const require = createRequire(import.meta.url);
    const utils: unknown = require("jsdom/lib/generated/idl/utils.js");
    if (!isRecord(utils) || typeof utils.implForWrapper !== "function") return null;
    const impl: unknown = (utils.implForWrapper as (wrapper: unknown) => unknown)(doc);
    if (
      !isRecord(impl) ||
      typeof impl._getDOMSelector !== "function" ||
      typeof impl._clearStyleCache !== "function"
    )
      return null;
    return impl as unknown as DocumentImpl;
  } catch {
    return null;
  }
}

interface Installed {
  impl: DocumentImpl;
  enabled: boolean;
}

const installed = new WeakMap<Document, Installed>();

/**
 * Installs the pre-filter for `doc` (once). Returns whether it is active:
 * `false` when jsdom's internals are not what this module expects.
 */
export function installFastStyles(doc: Document = document): boolean {
  if (installed.has(doc)) return true;
  const impl = documentImpl(doc);
  if (!impl) return false;
  const selector = impl._getDOMSelector();
  if (typeof selector.check !== "function") return false;
  const state: Installed = { impl, enabled: true };
  const original = selector.check.bind(selector);
  const required = new Map<string, Set<string> | null>();
  selector.check = (text, node, options) => {
    if (state.enabled) {
      let parts = required.get(text);
      if (parts === undefined) {
        const found = subjectParts(text);
        parts = found ? new Set(found) : null;
        required.set(text, parts);
      }
      if (parts !== null) {
        const part = node.getAttributeNS(null, "data-part");
        if (part === null || !parts.has(part)) return REJECTED;
      }
    }
    return original(text, node, options);
  };
  installed.set(doc, state);
  impl._clearStyleCache();
  return true;
}

/** Whether the pre-filter is installed and on for `doc`. */
export function fastStylesActive(doc: Document = document): boolean {
  return installed.get(doc)?.enabled ?? false;
}

/** Runs `fn` with the pre-filter switched off (for the equivalence test). */
export function withoutFastStyles<T>(fn: () => T, doc: Document = document): T {
  const state = installed.get(doc);
  if (!state) return fn();
  state.enabled = false;
  state.impl._clearStyleCache();
  try {
    return fn();
  } finally {
    state.enabled = true;
    state.impl._clearStyleCache();
  }
}
