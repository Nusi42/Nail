/**
 * The theme editor's token model (M5d WP183, ADR 0131): which group of the
 * editor a token category belongs to (colour, type, spacing, radius,
 * motion; everything else under "More"), the mode / variant properties a
 * token can carry, and the resolution of token values into CSS for the
 * editor's live preview — the edited theme's own values, with `$cat.name`
 * references (as written in KDL) and `var(--nail-…)` references (as the
 * compiled theme has them) followed through the same theme.
 *
 * Pure, and part of the app bundle (no mock code).
 */
import type { ThemeOverrides, ThemePartRule, ThemeTokenValue } from "../../api/generated";

/**
 * The Appearance page's overrides by the token they replace (the user's
 * `theme.kdl` `tokens`; they win over every theme, ADR 0102).
 */
export const OVERRIDE_TOKENS = {
  "color.accent": "accent",
  "font-size.body": "font_size",
  "radius.md": "radius",
} as const satisfies Readonly<Record<string, keyof ThemeOverrides>>;

export type OverriddenToken = keyof typeof OVERRIDE_TOKENS;

/** The overrides as token values by `category.name`. */
export function overrideTokens(overrides: ThemeOverrides): Map<OverriddenToken, string> {
  const out = new Map<OverriddenToken, string>();
  for (const [token, field] of Object.entries(OVERRIDE_TOKENS) as [
    OverriddenToken,
    keyof ThemeOverrides,
  ][]) {
    const value = overrides[field];
    if (value !== undefined) out.set(token, String(value));
  }
  return out;
}

/**
 * Drafts with the overrides laid over them (for the preview and the
 * contrast check: what the app will really show, ADR 0138).
 */
export function withOverrides(
  drafts: ReadonlyMap<string, string | null>,
  overrides: ReadonlyMap<string, string>,
  mode: string | null,
): Map<string, string | null> {
  const out = new Map(drafts);
  for (const [token, value] of overrides) {
    out.set(token, value);
    if (mode !== null) out.set(`${token}@${mode}`, value);
  }
  return out;
}

/** The editor's token groups, in tab order. */
export const TOKEN_GROUPS = ["color", "type", "spacing", "radius", "motion", "other"] as const;
export type TokenGroup = (typeof TOKEN_GROUPS)[number];

const GROUP_OF: Readonly<Record<string, TokenGroup>> = {
  color: "color",
  font: "type",
  "font-size": "type",
  "font-weight": "type",
  "line-height": "type",
  space: "spacing",
  size: "spacing",
  radius: "radius",
  motion: "motion",
};

/** The editor group of a token category (unknown categories: `other`). */
export function tokenGroup(category: string): TokenGroup {
  return GROUP_OF[category] ?? "other";
}

/**
 * The mode / variant properties of a token (`ThemePatch` `set-token`
 * `mode`); `null` is the base value.
 */
export const TOKEN_MODES = ["dark", "high-contrast", "print", "compact"] as const;
export type TokenMode = (typeof TOKEN_MODES)[number];

/** Categories whose unitless numbers are pixels (as in `nail-config`). */
export const PX_CATEGORIES = new Set(["font-size", "space", "size", "radius"]);

/** `color.accent` — how the editor names a token. */
export function tokenId(category: string, name: string): string {
  return `${category}.${name}`;
}

/** The draft key of a token value: `color.accent` or `color.accent@dark`. */
export function draftKey(category: string, name: string, mode: string | null): string {
  return mode === null ? tokenId(category, name) : `${tokenId(category, name)}@${mode}`;
}

/** A token's value in a mode as the theme defines it (base value when the mode has none). */
export function valueIn(token: ThemeTokenValue, mode: string | null): string {
  return mode === null ? token.value : (token.modes[mode] ?? token.value);
}

/** Whether the token has a value of its own for `mode` (base: always). */
export function hasModeValue(token: ThemeTokenValue, mode: string | null): boolean {
  return mode === null || Object.hasOwn(token.modes, mode);
}

/** A value as CSS: unitless numbers of pixel categories get `px`. */
export function cssValue(category: string, value: string): string {
  const trimmed = value.trim();
  if (PX_CATEGORIES.has(category) && /^-?\d+(\.\d+)?$/.test(trimmed)) return `${trimmed}px`;
  return trimmed;
}

/** `#abc` / `#aabbcc` → `#aabbcc` for a colour input; anything else → `null`. */
export function pickerColor(value: string): string | null {
  const v = value.trim();
  const short = /^#([0-9a-f])([0-9a-f])([0-9a-f])$/i.exec(v);
  if (short) {
    const [, r = "", g = "", b = ""] = short;
    return `#${r}${r}${g}${g}${b}${b}`.toLowerCase();
  }
  return /^#[0-9a-f]{6}$/i.test(v) ? v.toLowerCase() : null;
}

/** A pixel value (`14`, `14px`) as a number; anything else → `null`. */
export function pixels(value: string | undefined): number | null {
  const m = /^\s*(-?\d+(?:\.\d+)?)(?:px)?\s*$/.exec(value ?? "");
  return m ? Number(m[1]) : null;
}

const KDL_REF = /\$([a-z][a-z0-9-]*)\.([a-z][a-z0-9-]*)/g;
const VAR_REF = /var\((--nail-[a-z0-9-]+)(?:,\s*([^()]*))?\)/g;

/**
 * Every token of a theme resolved to CSS for one mode: the value of
 * `mode` (else the base value), unsaved drafts first, references followed
 * through the same theme (at most 8 levels; an unknown reference stays as
 * written). Keys are `category.name`.
 */
export function resolveTokenValues(
  tokens: readonly ThemeTokenValue[],
  mode: string | null,
  drafts: ReadonlyMap<string, string | null> = new Map(),
): Map<string, string> {
  const raw = new Map<string, { category: string; value: string }>();
  const byVar = new Map<string, string>();
  for (const token of tokens) {
    const key = tokenId(token.category, token.name);
    const base = drafts.get(key) ?? token.value;
    const modeDraft =
      mode === null ? undefined : drafts.get(draftKey(token.category, token.name, mode));
    const own = mode === null ? undefined : token.modes[mode];
    const value = modeDraft ?? own ?? base;
    raw.set(key, { category: token.category, value });
    byVar.set(`--nail-${token.category}-${token.name}`, key);
  }
  const out = new Map<string, string>();
  const resolve = (key: string, depth: number): string | undefined => {
    const done = out.get(key);
    if (done !== undefined) return done;
    const entry = raw.get(key);
    if (!entry) return undefined;
    let value = cssValue(entry.category, entry.value);
    if (depth < 8) {
      value = value
        .replace(KDL_REF, (whole, category: string, name: string) => {
          const ref = tokenId(category, name);
          return ref === key ? whole : (resolve(ref, depth + 1) ?? whole);
        })
        .replace(VAR_REF, (whole, name: string, fallback: string | undefined) => {
          const ref = byVar.get(name);
          const found = ref === undefined || ref === key ? undefined : resolve(ref, depth + 1);
          return found ?? fallback ?? whole;
        });
    }
    out.set(key, value);
    return value;
  };
  for (const key of raw.keys()) resolve(key, 0);
  return out;
}

/** One part selector of the editor with every rule that applies to it. */
export interface PartEntry {
  selector: string;
  rules: ThemePartRule[];
  /** Whether the theme has a rule of its own for it. */
  own: boolean;
}

/**
 * The theme's part rules grouped by selector (base rules first, as the
 * details list them), plus `extra` selectors without a rule (a part the
 * inspector jumped to, a part the user added); sorted by selector.
 */
export function partEntries(
  parts: readonly ThemePartRule[],
  extra: readonly string[] = [],
): PartEntry[] {
  const map = new Map<string, PartEntry>();
  for (const rule of parts) {
    const entry = map.get(rule.selector) ?? { selector: rule.selector, rules: [], own: false };
    entry.rules.push(rule);
    if (rule.origin === "own") entry.own = true;
    map.set(rule.selector, entry);
  }
  for (const selector of extra) {
    if (selector !== "" && !map.has(selector))
      map.set(selector, { selector, rules: [], own: false });
  }
  return [...map.values()].sort((a, b) => a.selector.localeCompare(b.selector));
}

/** The part name a selector targets (`split[x="y"] split.divider:hover` → `split.divider`). */
export function selectorPart(selector: string): string {
  const last = selector.trim().split(/\s+/).at(-1) ?? "";
  return /^[a-z0-9.-]*/.exec(last)?.[0] ?? "";
}

/** Whether a selector targets `part` (or one of its sub-parts when `prefix`). */
export function selectorMatches(selector: string, part: string, prefix = false): boolean {
  const target = selectorPart(selector);
  return target === part || (prefix && target.startsWith(`${part}.`));
}

/**
 * A plausible part selector as the user typed it: part names (lowercase
 * kebab segments joined by dots), optional `[key="value"]` / `[key]`
 * conditions and `:state`s, ancestors separated by spaces. The core checks
 * it for real (`edit-issue-invalid-part`).
 */
export function isPartSelector(selector: string): boolean {
  const step =
    /^[a-z][a-z0-9-]*(?:\.[a-z][a-z0-9-]*)*(?:\[[a-z][a-z0-9-]*(?:~?="[^"\\]*")?\]|:[a-z][a-z-]*)*$/;
  const steps = selector.trim().split(/\s+/);
  return steps.length > 0 && steps.every((s) => step.test(s));
}

/** A CSS property name as theme rules use them (`background-color`). */
export function isPropertyName(property: string): boolean {
  return /^[a-z][a-z-]*$/.test(property.trim());
}
