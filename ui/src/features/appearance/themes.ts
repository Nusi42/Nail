/**
 * The built-in themes for the mock core (`?mock=1`, the UI tests): the
 * shipped `defaults/themes/*.kdl` files, bundled as text, with their
 * `tokens` read by a small reader and laid over the base theme like the core
 * resolves them (ADR 0102): a token a theme sets replaces the base value and
 * drops the base's mode variants unless the theme gives its own; `compact=`
 * is the density variant. Part rules of the theme files are left to the real
 * core — the mock only needs the colours, sizes and modes to switch.
 *
 * Reading the real files keeps the mock in step with WP151's palettes.
 */
import type {
  AppearanceSettings,
  Density,
  PartCondition,
  PartRule,
  ResolvedTheme,
  ThemeInfo,
  ThemeKind,
  ThemeMode,
  TokenSet,
} from "../../api/generated";
import { PX_CATEGORIES } from "./tokens";

const files = import.meta.glob<string>("../../../../defaults/themes/*.kdl", {
  query: "?raw",
  import: "default",
  eager: true,
});

/** Categories whose unitless numbers are pixels (as in `nail-config`). */
/** Token variants a theme file may give. */
const VARIANTS = new Set(["light", "dark", "high-contrast", "print", "compact"]);

/** One theme file's metadata and tokens. */
export interface ThemeFile {
  id: string;
  name: string;
  mode: ThemeMode;
  tokens: TokenSet;
  /** Variant → tokens (`dark`, `high-contrast`, `print`, `compact`). */
  variants: Record<string, TokenSet>;
}

/** `$color.accent` → `var(--nail-color-accent)`. */
function refs(value: string): string {
  return value.replace(/\$([a-z][a-z0-9-]*)\.([a-z][a-z0-9-]*)/g, "var(--nail-$1-$2)");
}

/** A KDL value: `"…"`, `#"…"#` or a bare number / word. Returns [value, rest]. */
function readValue(text: string): [string, string] | null {
  const s = text.trimStart();
  const raw = /^#"([\s\S]*?)"#/.exec(s);
  if (raw) return [raw[1] ?? "", s.slice(raw[0].length)];
  const quoted = /^"((?:[^"\\]|\\.)*)"/.exec(s);
  if (quoted) return [(quoted[1] ?? "").replace(/\\(.)/g, "$1"), s.slice(quoted[0].length)];
  const bare = /^([^\s=;{}]+)(?=\s|$|;)/.exec(s);
  if (bare) return [bare[1] ?? "", s.slice(bare[0].length)];
  return null;
}

export function tokenValue(category: string, value: string): string {
  if (PX_CATEGORIES.has(category) && /^-?\d+(\.\d+)?$/.test(value)) return `${value}px`;
  return refs(value);
}

function put(set: TokenSet, category: string, name: string, value: string): void {
  (set[category] ??= {})[name] = value;
}

/** Reads the metadata and `tokens { … }` block of a theme file. */
export function parseThemeFile(id: string, text: string): ThemeFile {
  const header = /^\s*theme\s+"([^"]*)"([^{\n]*)\{/m.exec(text);
  const mode = /\bmode="([a-z-]+)"/.exec(header?.[2] ?? "")?.[1];
  const file: ThemeFile = {
    id,
    name: header?.[1] ?? id,
    mode: mode === "light" || mode === "dark" || mode === "high-contrast" ? mode : "system",
    tokens: {},
    variants: {},
  };
  const start = text.search(/^\s*tokens\s*\{/m);
  if (start < 0) return file;
  const lines = text.slice(start).split("\n").slice(1);
  for (const rawLine of lines) {
    const line = rawLine.replace(/(^|\s)\/\/.*$/, "").trim();
    if (line.startsWith("}")) break;
    const m = /^([a-z][a-z-]*)\s+"([a-z][a-z0-9.-]*)"\s+(.*)$/.exec(line);
    if (!m) continue;
    const category = m[1] ?? "";
    const name = m[2] ?? "";
    const first = readValue(m[3] ?? "");
    if (!first) continue;
    put(file.tokens, category, name, tokenValue(category, first[0]));
    let rest = first[1];
    for (;;) {
      const prop = /^\s*([a-z-]+)=/.exec(rest);
      if (!prop) break;
      const value = readValue(rest.slice(prop[0].length));
      if (!value) break;
      const variant = prop[1] ?? "";
      if (VARIANTS.has(variant)) {
        put((file.variants[variant] ??= {}), category, name, tokenValue(category, value[0]));
      }
      rest = value[1];
    }
  }
  return file;
}

/** The built-in theme files by id (`frost`, `sage`, …). */
export const BUILTIN_THEME_FILES: ReadonlyMap<string, ThemeFile> = new Map(
  Object.entries(files)
    .map(([path, text]) => {
      const id = /([^/\\]+)\.kdl$/.exec(path)?.[1] ?? path;
      return [id, parseThemeFile(id, text)] as const;
    })
    .sort(([a], [b]) => a.localeCompare(b)),
);

/** The built-in theme a user theme of each kind previews with when its chain names none. */
const PREVIEW_FALLBACK: Readonly<Record<ThemeKind, string>> = {
  light: "frost",
  dark: "graphite",
  "high-contrast": "high-contrast",
};

/**
 * The colour tokens a gallery card previews `theme` with: a built-in's own
 * palette; for a user theme the palette of the built-in it extends
 * (following `extends` through other user themes), else of the built-in of
 * its kind — never an empty strip. The theme's own resolved colours from the
 * core (`ThemeInfo.preview`, M5b) win over that palette, so a user theme with
 * its own accent shows that accent.
 */
export function previewColors(
  theme: ThemeInfo,
  themes: readonly ThemeInfo[],
): Readonly<Record<string, string>> {
  const base = basePreviewColors(theme, themes);
  const own = theme.preview;
  if (!own) return base;
  const merged: Record<string, string> = { ...base };
  for (const [token, value] of Object.entries(own)) {
    merged[token] = value;
  }
  return merged;
}

/** The colours of the nearest built-in in `theme`'s chain (see `previewColors`). */
function basePreviewColors(
  theme: ThemeInfo,
  themes: readonly ThemeInfo[],
): Readonly<Record<string, string>> {
  const seen = new Set<string>();
  let id: string | undefined = theme.id;
  while (id !== undefined && !seen.has(id)) {
    seen.add(id);
    const builtin = BUILTIN_THEME_FILES.get(id);
    if (builtin) return builtin.tokens.color ?? {};
    const at: string = id;
    id = at === theme.id ? theme.extends : themes.find((t) => t.id === at)?.extends;
  }
  return BUILTIN_THEME_FILES.get(PREVIEW_FALLBACK[theme.kind])?.tokens.color ?? {};
}

/**
 * `base` with the theme `id` laid over it, the user's density and overrides
 * applied — what the core's `GetConfig` returns for that selection.
 */
export function resolveMockTheme(
  base: ResolvedTheme,
  id: string,
  settings: Pick<AppearanceSettings, "density" | "overrides">,
  pristine: ResolvedTheme | null = null,
  layers: readonly ThemeLayer[] = [],
): ResolvedTheme {
  const theme = structuredClone(base);
  const file = BUILTIN_THEME_FILES.get(id);
  const replace = (category: string, name: string, value: string) => {
    put(theme.tokens, category, name, value);
    for (const tokens of Object.values(theme.modes)) delete tokens[category]?.[name];
  };
  /** The user did not change this token (it is still the shipped default). */
  const untouched = (category: string, name: string) =>
    pristine === null || pristine.tokens[category]?.[name] === base.tokens[category]?.[name];
  if (file) {
    theme.name = file.name;
    theme.mode = file.mode;
    for (const [category, set] of Object.entries(file.tokens)) {
      for (const [name, value] of Object.entries(set)) {
        if (untouched(category, name)) replace(category, name, value);
      }
    }
    for (const [variant, set] of Object.entries(file.variants)) {
      for (const [category, values] of Object.entries(set)) {
        for (const [name, value] of Object.entries(values)) {
          if (untouched(category, name)) put((theme.modes[variant] ??= {}), category, name, value);
        }
      }
    }
  }
  // User themes over the built-in (M5d), then the user's own theme.kdl edits.
  for (const layer of layers) {
    for (const [category, set] of Object.entries(layer.tokens)) {
      for (const [name, value] of Object.entries(set)) replace(category, name, value);
    }
    for (const [variant, set] of Object.entries(layer.variants)) {
      for (const [category, values] of Object.entries(set)) {
        for (const [name, value] of Object.entries(values)) {
          put((theme.modes[variant] ??= {}), category, name, value);
        }
      }
    }
    theme.parts.push(...structuredClone(layer.parts));
  }
  const o = settings.overrides;
  if (o.font_size !== undefined) replace("font-size", "body", `${String(o.font_size)}px`);
  if (o.radius !== undefined) replace("radius", "md", `${String(o.radius)}px`);
  if (o.accent !== undefined) replace("color", "accent", o.accent);
  const density: Density = settings.density;
  theme.density = density;
  return theme;
}

/** A token as a theme file of the mock writes it (`value` as in KDL). */
export interface MockThemeToken {
  category: string;
  name: string;
  /** The base value; absent = only mode values. */
  value?: string;
  /** Mode / variant values (`dark`, `high-contrast`, `print`, `compact`). */
  modes: Record<string, string>;
}

/** A part rule of a theme file of the mock. */
export interface MockPartRule {
  selector: string;
  properties: Record<string, string>;
}

/** What a theme file of the mock defines itself (a user theme, or `theme.kdl`). */
export interface MockThemeEdits {
  tokens: MockThemeToken[];
  parts: MockPartRule[];
}

/** One theme laid over the base by `resolveMockTheme` (CSS values). */
export interface ThemeLayer {
  tokens: TokenSet;
  variants: Record<string, TokenSet>;
  parts: PartRule[];
}

/** `split[orientation="horizontal"] split.divider:hover` → target, states, conditions, ancestors. */
export function parseSelector(
  selector: string,
): Pick<PartRule, "selector" | "states" | "conditions" | "ancestors"> {
  const steps = selector
    .trim()
    .split(/\s+/)
    .map((step) => {
      const name = /^[a-z0-9.-]*/.exec(step)?.[0] ?? "";
      const states = [...step.matchAll(/:([a-z][a-z-]*)/g)].map((m) => m[1] ?? "");
      const conditions: PartCondition[] = [
        ...step.matchAll(/\[([a-z][a-z0-9-]*)(?:~?="([^"]*)")?\]/g),
      ].map((m) => ({ key: m[1] ?? "", value: m[2] ?? null }));
      return { selector: name, states, conditions };
    });
  const target = steps.pop() ?? { selector: "", states: [], conditions: [] };
  return { ...target, ancestors: steps };
}

/** A theme file's own tokens and parts as a layer (CSS values, `$refs` → `var()`). */
export function layerOf(edits: MockThemeEdits, file: string): ThemeLayer {
  const layer: ThemeLayer = { tokens: {}, variants: {}, parts: [] };
  for (const token of edits.tokens) {
    if (token.value !== undefined) {
      put(layer.tokens, token.category, token.name, tokenValue(token.category, token.value));
    }
    for (const [mode, value] of Object.entries(token.modes)) {
      put(
        (layer.variants[mode] ??= {}),
        token.category,
        token.name,
        tokenValue(token.category, value),
      );
    }
  }
  edits.parts.forEach((rule, index) => {
    layer.parts.push({
      ...parseSelector(rule.selector),
      declarations: Object.fromEntries(
        Object.entries(rule.properties).map(([property, value]) => [property, refs(value)]),
      ),
      source: { file, line: index + 1, column: 1 },
    });
  });
  return layer;
}
