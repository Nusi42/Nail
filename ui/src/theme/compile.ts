/**
 * Theme compiler: `ResolvedTheme` → one stylesheet (design.md, "UI
 * customization"; ADR 0003, "Part selectors → DOM").
 *
 * Output, in order:
 *   1. structural base rules (layout containers must work with any theme),
 *   2. tokens as custom properties on `:root`, with per-mode overrides keyed on
 *      `:root[data-nail-mode=…]`, so a mode switch only flips an attribute
 *      (the ThemeProvider sets it from the theme / user override and, for
 *      `system`, from `prefers-color-scheme` / `prefers-contrast`),
 *   3. reader rules from the theme's `message` section,
 *   4. part rules in source order,
 *   5. raw-css blocks.
 *
 * Everything is validated defensively: a rule with an unsafe selector piece or
 * value is dropped with a warning instead of corrupting the sheet.
 */
import type {
  AncestorSelector,
  MessageDisplay,
  PartCondition,
  PartRule,
  ResolvedTheme,
  ThemeMode,
  TokenSet,
} from "../api/generated";

/** States that map to CSS pseudo-classes; all others to `[data-state~=…]`. */
const PSEUDO_STATES = new Set(["hover", "active", "focus", "focus-visible", "focus-within"]);

const PART_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*(?:\.[a-z0-9]+(?:-[a-z0-9]+)*)*$/;
const IDENT_RE = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const PROPERTY_RE = /^-?[a-z][a-z0-9-]*$/;

export class ThemeCompileError extends Error {}

/** Quotes a string as a CSS string literal. */
export function cssString(value: string): string {
  return `"${value.replace(/[\\"]/g, "\\$&").replace(/[\n\r\f]/g, " ")}"`;
}

/**
 * Checks that a declaration value cannot escape its declaration or load
 * remote content. Values are produced by `nail-config`, which validates them
 * too; this is defence in depth.
 */
export function isSafeValue(value: string): boolean {
  if (/[{};<>\\]/.test(value) || value.includes("/*")) {
    return false;
  }
  return !/(url\s*\(|@import|expression\s*\(|image-set\s*\(|javascript:)/i.test(value);
}

function checkIdent(kind: string, value: string, re: RegExp): string {
  if (!re.test(value)) {
    throw new ThemeCompileError(`invalid ${kind} "${value}"`);
  }
  return value;
}

function conditionSelector(condition: PartCondition): string {
  const key = checkIdent("condition key", condition.key, IDENT_RE);
  if (condition.value === null) {
    return `[data-${key}]`;
  }
  return `[data-${key}~=${cssString(condition.value)}]`;
}

/** Selector for one element: part name + states + conditions. */
export function elementSelector(
  part: string,
  states: readonly string[],
  conditions: readonly PartCondition[],
): string {
  let selector = `[data-part=${cssString(checkIdent("part name", part, PART_RE))}]`;
  for (const state of states) {
    checkIdent("state", state, IDENT_RE);
    selector += PSEUDO_STATES.has(state) ? `:${state}` : `[data-state~=${cssString(state)}]`;
  }
  for (const condition of conditions) {
    selector += conditionSelector(condition);
  }
  return selector;
}

/** Full CSS selector of a part rule, ancestors included. */
export function ruleSelector(
  rule: Pick<PartRule, "selector" | "states" | "conditions" | "ancestors">,
): string {
  const chain = rule.ancestors.map((a: AncestorSelector) =>
    elementSelector(a.selector, a.states, a.conditions),
  );
  chain.push(elementSelector(rule.selector, rule.states, rule.conditions));
  return chain.join(" ");
}

function declarationBlock(declarations: Record<string, string>): string {
  const lines: string[] = [];
  for (const [property, value] of Object.entries(declarations)) {
    checkIdent("property", property, PROPERTY_RE);
    if (!isSafeValue(value)) {
      throw new ThemeCompileError(`unsafe value for ${property}: ${value}`);
    }
    lines.push(`  ${property}: ${value};`);
  }
  return lines.join("\n");
}

/** Custom-property name of a token: `--nail-<category>-<name>`. */
export function tokenVar(category: string, name: string): string {
  return `--nail-${checkIdent("token category", category, IDENT_RE)}-${checkIdent("token name", name, IDENT_RE)}`;
}

function tokenDeclarations(tokens: TokenSet, warnings: string[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [category, set] of Object.entries(tokens)) {
    for (const [name, value] of Object.entries(set)) {
      try {
        if (!isSafeValue(value)) {
          throw new ThemeCompileError(`unsafe token value ${category}.${name}`);
        }
        out[tokenVar(category, name)] = value;
      } catch (error) {
        warnings.push(String((error as Error).message));
      }
    }
  }
  return out;
}

function block(selector: string, declarations: Record<string, string>): string {
  const body = Object.entries(declarations)
    .map(([p, v]) => `  ${p}: ${v};`)
    .join("\n");
  return `${selector} {\n${body}\n}`;
}

/** Structural rules every theme gets; themes may override any of them. */
export const BASE_CSS = `*, *::before, *::after { box-sizing: border-box; }
html, body { margin: 0; height: 100%; }
[data-part="app"] { position: fixed; inset: 0; display: flex; flex-direction: column; overflow: hidden; }
[data-part="app"] > * { flex: 1 1 auto; min-height: 0; }
[data-part="split"] { display: flex; flex-direction: row; min-width: 0; min-height: 0; overflow: hidden; }
[data-part="split"][data-orientation="vertical"] { flex-direction: column; }
[data-part="split.pane"] { display: flex; flex-direction: column; flex: 1 1 0px; min-width: 0; min-height: 0; overflow: hidden; }
[data-part="split.pane"] > * { flex: 1 1 auto; min-height: 0; }
[data-part="split.divider"] { flex: 0 0 4px; cursor: col-resize; touch-action: none; }
[data-part="split"][data-orientation="vertical"] > [data-part="split.divider"] { cursor: row-resize; }
[data-part="stack"] { display: grid; grid-template-columns: minmax(0, 1fr); min-width: 0; min-height: 0; overflow: hidden; }
[data-part="stack"] > * { min-height: 0; min-width: 0; }
[data-part="tabs"] { display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; }
[data-part="tabs.tab"] { font: inherit; border: none; cursor: default; }
[data-part="tabs.panel"] { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; }
[data-part="tabs.panel"] > * { flex: 1 1 auto; min-height: 0; }
[hidden] { display: none !important; }
[data-part="toolbar.button"] { font: inherit; }
[data-part="account-switcher.item"], [data-part="folder-tree.item"] { font: inherit; }
[data-part="message-list"] { overflow-y: auto; }
[data-part="message-list.row"] { outline-offset: -2px; }
[data-part="message-list.row.line"] > * { overflow: hidden; text-overflow: ellipsis; }
[data-part="message-list.row.chip"] { display: inline-block; }
[data-emphasis~="strong"] { font-weight: var(--nail-font-weight-bold, bold); }
[data-part="message-view.body"] { display: block; width: 100%; min-height: 0; }
[data-part="message-view.text"] { flex: 1 1 auto; margin: 0; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; }
[data-part="folder-tree.list"], [data-part="account-switcher"] { list-style: none; margin: 0; }
[data-part="folder-tree.list"] { padding: 0; }
[data-part="message-view.header.field"] { display: flex; gap: 0.5em; }
[data-part="window-layer"] { position: fixed; inset: 0; display: flex; align-items: center; justify-content: center; }
[data-part="window"] { display: flex; flex-direction: column; max-width: 100%; max-height: 100%; min-height: 0; overflow: hidden; }
[data-part="window"]:not([data-state~="active"]) { display: none; }
[data-part="window.header"] { display: flex; align-items: center; flex: 0 0 auto; }
[data-part="window.title"] { flex: 1 1 auto; margin: 0; font: inherit; }
[data-part="window.body"] { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; overflow: auto; }
[data-part="window.body"] > * { flex: 1 1 auto; min-height: 0; }
button, input, select, textarea { font: inherit; }
[data-part="thread-view.body"], [data-part="thread-view.quote"] { white-space: pre-wrap; overflow-wrap: anywhere; margin: 0; }
[data-part="search-bar.hints"], [data-part="thread-view.list"], [data-part="outbox.list"], [data-part="folder-picker.list"] { list-style: none; margin: 0; padding: 0; }
${[
  "composer.suggestions",
  "composer.attachments.list",
  "rules.list.items",
  "rules.group.items",
  "rules.actions.items",
  "rules.test.matches",
  "rules.test.actions",
  "rules.runs.items",
  "mcp-clients.list",
  "inspector.path",
  "inspector.states",
  "inspector.rules",
  "merge-dialog.items",
  "merge-dialog.conflicts",
  "merge-dialog.backup-list",
  "config-errors.list",
  "smart-folders.list",
]
  .map((part) => `[data-part="${part}"]`)
  .join(", ")} { list-style: none; }
[data-part="mcp-audit.table"] { border-collapse: collapse; }
[data-part="search-bar.error.query"] { white-space: pre; }
[data-part="search-bar.input"]::-webkit-search-cancel-button { display: none; }
${[1, 2, 3, 4, 5, 6].map((d) => `[data-part="folder-tree.item"][data-depth="${d}"], [data-part="folder-picker.item"][data-depth="${d}"] { margin-inline-start: ${d * 16}px; }`).join("\n")}
[data-part="context-menu"] { position: fixed; z-index: 1000; }
[data-part="context-menu.item"] { display: flex; align-items: center; width: 100%; font: inherit; text-align: start; }
[data-part="context-menu.submenu"] { position: relative; }
[data-part="context-menu.popup"] { position: absolute; inset-inline-start: 100%; top: 0; }
[data-part="message-list.row.columns"], [data-part="message-list.header"] { display: grid; grid-auto-flow: column; grid-auto-columns: minmax(0, 1fr); }
[data-part="message-list.row.cell"] { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
${[
  "toolbar-editor.items",
  "column-editor.items",
  "snooze.presets",
  "snooze-list.items",
  "notification-settings.accounts",
  "sieve-status.issues",
  "vacation-editor.issues",
  "junk-settings.issues",
  "notification-settings.issues",
  "sync-policy.issues",
]
  .map((part) => `[data-part="${part}"]`)
  .join(", ")} { list-style: none; margin: 0; padding: 0; }
/* Reduced motion (M2c): the OS setting stops animations and transitions. */
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after { animation-duration: 0.01ms !important; animation-iteration-count: 1 !important; transition-duration: 0.01ms !important; transition-delay: 0s !important; scroll-behavior: auto !important; }
}`;

function readerRules(message: MessageDisplay): string {
  const decls: Record<string, string> = {
    "font-family": message.text_font_family,
    "font-size": message.font_size,
    "line-height": message.line_height,
    "max-width": message.max_width,
  };
  for (const value of Object.values(decls)) {
    if (!isSafeValue(value)) {
      throw new ThemeCompileError("unsafe value in the message section");
    }
  }
  return block('[data-part="message-view.text"]', decls);
}

/** Which colour scheme a mode renders with (for `color-scheme`). */
function colorScheme(mode: string): string {
  return mode === "dark" || mode === "high-contrast" ? "dark" : "light";
}

function tokenRules(theme: ResolvedTheme, warnings: string[]): string[] {
  const out: string[] = [];
  out.push(
    block(":root", { ...tokenDeclarations(theme.tokens, warnings), "color-scheme": "light" }),
  );
  for (const [mode, tokens] of Object.entries(theme.modes)) {
    try {
      checkIdent("mode", mode, IDENT_RE);
    } catch (error) {
      warnings.push((error as Error).message);
      continue;
    }
    const decls = { ...tokenDeclarations(tokens, warnings), "color-scheme": colorScheme(mode) };
    out.push(block(`:root[data-nail-mode=${cssString(mode)}]`, decls));
  }
  return out;
}

export interface CompiledTheme {
  css: string;
  /** Rules that were dropped, with the reason. */
  warnings: string[];
}

/** Compiles a resolved theme into one stylesheet. */
export function compileTheme(theme: ResolvedTheme): CompiledTheme {
  const warnings: string[] = [];
  const sections: string[] = [BASE_CSS, ...tokenRules(theme, warnings)];
  try {
    sections.push(readerRules(theme.message));
  } catch (error) {
    warnings.push((error as Error).message);
  }
  for (const rule of theme.parts) {
    try {
      sections.push(`${ruleSelector(rule)} {\n${declarationBlock(rule.declarations)}\n}`);
    } catch (error) {
      const where = rule.source ? ` (${rule.source.file}:${rule.source.line})` : "";
      warnings.push(`${(error as Error).message}${where}`);
    }
  }
  for (const raw of theme.raw_css) {
    try {
      const selector = elementSelector(raw.selector, [], []);
      if (!isSafeValue(raw.css.replaceAll(";", ""))) {
        throw new ThemeCompileError(`unsafe raw-css for ${raw.selector}`);
      }
      sections.push(`${selector} {\n  ${raw.css.trim()}\n}`);
    } catch (error) {
      warnings.push((error as Error).message);
    }
  }
  return { css: `${sections.join("\n")}\n`, warnings };
}

/** The concrete mode to show for a theme / override and the OS preferences. */
export function effectiveMode(
  mode: ThemeMode,
  prefersDark: boolean,
  prefersContrast: boolean,
): "light" | "dark" | "high-contrast" {
  if (mode !== "system") {
    return mode;
  }
  if (prefersContrast) {
    return "high-contrast";
  }
  return prefersDark ? "dark" : "light";
}

/**
 * Replaces `var(--nail-…)` references by token values for `mode`, for
 * documents that cannot see the app's custom properties (the reader iframe).
 */
export function resolveTokenRefs(value: string, theme: ResolvedTheme, mode: string): string {
  const lookup = new Map<string, string>();
  const add = (tokens: TokenSet | undefined) => {
    for (const [category, set] of Object.entries(tokens ?? {})) {
      for (const [name, v] of Object.entries(set)) {
        lookup.set(`--nail-${category}-${name}`, v);
      }
    }
  };
  add(theme.tokens);
  add(theme.modes[mode]);
  let current = value;
  for (let depth = 0; depth < 8 && current.includes("var(--nail-"); depth++) {
    current = current.replace(
      /var\((--nail-[a-z0-9-]+)(?:,\s*([^()]*))?\)/g,
      (_m, name: string, fallback?: string) => {
        return lookup.get(name) ?? fallback ?? "initial";
      },
    );
  }
  return current;
}
