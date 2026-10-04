/**
 * Key binding matching (layout `keys`): chords like `ctrl+shift+m`,
 * sequences like `g i`, per context with `global` as the fallback.
 */
import type { JsonValue, KeyBinding } from "../api/generated";
import { charCount } from "../text/chars";

const MODIFIER_KEYS = new Set(["control", "shift", "alt", "meta", "altgraph", "capslock", "os"]);
const ALIASES: Record<string, string> = { esc: "escape", del: "delete", space: " ", spacebar: " " };

/** The subset of `KeyboardEvent` needed to build a combo. */
export interface KeyLike {
  key: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

function canonicalKey(key: string): string {
  const lower = key.toLowerCase();
  return ALIASES[lower] ?? lower;
}

/**
 * Normalizes an event to a combo string (`ctrl+shift+m`, `j`, `arrowdown`).
 * Returns `null` for lone modifier presses. Shift is kept for letters and
 * named keys but not for symbols whose shifted character is the key itself
 * (`?`).
 */
export function comboFromEvent(event: KeyLike): string | null {
  const key = canonicalKey(event.key);
  if (MODIFIER_KEYS.has(key) || key === "dead" || key === "unidentified") return null;
  const parts: string[] = [];
  if (event.ctrlKey) parts.push("ctrl");
  if (event.altKey) parts.push("alt");
  const isSymbol = charCount(key) === 1 && !/[a-z0-9]/.test(key);
  if (event.shiftKey && !isSymbol) parts.push("shift");
  if (event.metaKey) parts.push("meta");
  parts.push(key);
  return parts.join("+");
}

/** Normalizes a combo written in the layout file (`Ctrl+Shift+M` → `ctrl+shift+m`). */
export function normalizeCombo(combo: string): string {
  const pieces = combo.split("+").map((p) => p.trim());
  const key = canonicalKey(pieces.pop() ?? "");
  const mods = new Set(
    pieces.map((m) =>
      canonicalKey(m)
        .replace(/^control$/, "ctrl")
        .replace(/^cmd$/, "meta"),
    ),
  );
  const ordered = ["ctrl", "alt", "shift", "meta"].filter((m) => mods.has(m));
  return [...ordered, key === "" && combo.endsWith("+") ? "+" : key].join("+");
}

interface CompiledBinding {
  steps: string[];
  context: string;
  action: string;
  args: JsonValue[];
  order: number;
}

export interface KeyMatch {
  action: string;
  args: JsonValue[];
}

export type FeedResult =
  { kind: "match"; match: KeyMatch } | { kind: "pending" } | { kind: "none" };

/**
 * Stateful matcher: feed it one combo per key press together with the active
 * contexts (most specific first, e.g. `["list", "global"]`).
 */
export class KeyMatcher {
  private readonly bindings: CompiledBinding[];
  private buffer: string[] = [];
  private pendingExact: KeyMatch | null = null;

  constructor(bindings: readonly KeyBinding[]) {
    this.bindings = bindings
      .filter((b) => b.key.trim() !== "")
      .map((b, order) => ({
        steps: b.key.trim().split(/\s+/).map(normalizeCombo),
        context: b.context,
        action: b.action,
        args: b.args,
        order,
      }));
  }

  /** Whether a sequence is in progress. */
  get pending(): boolean {
    return this.buffer.length > 0;
  }

  /** Abandons a pending sequence, returning an exact match it was waiting on. */
  flush(): KeyMatch | null {
    const match = this.pendingExact;
    this.reset();
    return match;
  }

  reset(): void {
    this.buffer = [];
    this.pendingExact = null;
  }

  feed(combo: string, contexts: readonly string[]): FeedResult {
    const attempt = [...this.buffer, combo];
    const result = this.match(attempt, contexts);
    if (result.kind === "none" && this.buffer.length > 0) {
      // The sequence broke: retry the key on its own.
      this.reset();
      return this.feed(combo, contexts);
    }
    if (result.kind === "pending") {
      this.buffer = attempt;
    } else {
      this.reset();
    }
    return result;
  }

  private match(steps: string[], contexts: readonly string[]): FeedResult {
    const rank = (b: CompiledBinding) => contexts.indexOf(b.context);
    const candidates = this.bindings.filter(
      (b) => rank(b) >= 0 && steps.every((s, i) => b.steps[i] === s),
    );
    if (candidates.length === 0) return { kind: "none" };
    const exact = candidates
      .filter((b) => b.steps.length === steps.length)
      // Most specific context first; within a context, later bindings win.
      .sort((a, b) => rank(a) - rank(b) || b.order - a.order)[0];
    const longer = candidates.some((b) => b.steps.length > steps.length);
    if (longer) {
      this.pendingExact = exact ? { action: exact.action, args: exact.args } : null;
      return { kind: "pending" };
    }
    return exact
      ? { kind: "match", match: { action: exact.action, args: exact.args } }
      : { kind: "none" };
  }
}
