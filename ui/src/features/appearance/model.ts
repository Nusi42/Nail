/**
 * The appearance rules the UI needs outside the mock core: the built-in
 * theme ids, the override ranges and their validation, and which theme is
 * active. The constants are generated; the rules are ports of
 * `nail_api::appearance` pinned by `crates/nail-api/tests/ts_vectors.rs`
 * (vectors run in `model.test.ts`); the mock core
 * (`./mock.ts`) answers the requests with them. Kept apart from the mock so
 * the app bundle does not carry the mock core.
 */
import {
  type AppearanceSettings,
  DEFAULT_DARK_THEME,
  MAX_THEME_ID_CHARS,
  MAX_THEME_NAME_CHARS,
  DEFAULT_LIGHT_THEME,
  FONT_SIZE_RANGE,
  HIGH_CONTRAST_THEME,
  RADIUS_RANGE,
  type SystemAppearance,
  type ThemeInfo,
  type ThemeOverrides,
  type ThemeOverridesIssue,
} from "../../api/generated";
import { charCount } from "../../text/chars";
export {
  BUILTIN_THEMES,
  DEFAULT_DARK_THEME,
  DEFAULT_LIGHT_THEME,
  FONT_SIZE_RANGE,
  HIGH_CONTRAST_THEME,
  RADIUS_RANGE,
} from "../../api/generated";

/** `nail_api::appearance::is_hex_color`. */
export function isHexColor(value: string): boolean {
  return /^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6})$/.test(value);
}

/** `ThemeOverrides::issues`, in field order. */
export function overrideIssues(o: ThemeOverrides): ThemeOverridesIssue[] {
  const out: ThemeOverridesIssue[] = [];
  const outside = (v: number | undefined, [min, max]: readonly [number, number]) =>
    v !== undefined && (v < min || v > max);
  if (outside(o.font_size, FONT_SIZE_RANGE)) out.push("font-size-range");
  if (outside(o.radius, RADIUS_RANGE)) out.push("radius-range");
  if (o.accent !== undefined && !isHexColor(o.accent)) out.push("accent-invalid");
  return out;
}

/** `nail_api::appearance::active_theme`. */
export function activeTheme(
  settings: AppearanceSettings,
  system: SystemAppearance | undefined,
  themes: readonly ThemeInfo[],
): string {
  const usable = (id: string) => themes.some((t) => t.id === id && t.problem === undefined);
  const pick = (id: string, fallback: string) => (usable(id) ? id : fallback);
  if (settings.follow_system) {
    if (system?.high_contrast && usable(HIGH_CONTRAST_THEME)) return HIGH_CONTRAST_THEME;
    if (system?.scheme === "dark") return pick(settings.dark, DEFAULT_DARK_THEME);
    return pick(settings.light, DEFAULT_LIGHT_THEME);
  }
  const kind = themes.find((t) => t.id === settings.theme)?.kind;
  return pick(settings.theme, kind === "dark" ? DEFAULT_DARK_THEME : DEFAULT_LIGHT_THEME);
}

/**
 * `nail_api::appearance::accent_ignored` (ADR 0161): the accent override is
 * left out while the active theme is a high-contrast one (its kind, which a
 * user theme inherits from the theme it extends).
 */
export function accentIgnored(active: string, themes: readonly ThemeInfo[]): boolean {
  return themes.find((t) => t.id === active)?.kind === "high-contrast";
}

/** A theme id (file stem): lowercase kebab starting with a letter (`nail_api::appearance::is_theme_id`). */
export function isThemeId(id: string): boolean {
  return /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/.test(id) && id.length <= MAX_THEME_ID_CHARS;
}

/**
 * The id the core derives from a theme name (`id_from_name` in
 * `crates/nail-config/src/theme_edit.rs`; pinned by
 * `crates/nail-config/src/ts_vectors.rs`): `My Ocean!` → `my-ocean`,
 * `Café` → `cafe`, `2024` → `theme-2024`, nothing usable → `theme`; room
 * is left for a `-<n>` suffix.
 */
export function themeIdFromName(name: string): string {
  let id = name
    .normalize("NFKD")
    .replace(/\p{M}/gu, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  if (!/^[a-z]/.test(id)) id = id === "" ? "theme" : `theme-${id}`;
  return id.slice(0, MAX_THEME_ID_CHARS - 4).replace(/-+$/, "");
}

/**
 * Whether a name is one the core accepts (`nail_api::editing::is_theme_name`):
 * not blank, at most `MAX_THEME_NAME_CHARS` characters, no control
 * characters.
 */
export function isThemeName(name: string): boolean {
  return name.trim() !== "" && charCount(name) <= MAX_THEME_NAME_CHARS && !/\p{Cc}/u.test(name);
}

/**
 * How to make `theme` the one in use without losing Follow Windows: with
 * Follow Windows on, a light or dark theme takes its half of the pair
 * (`SetThemePreferences`); otherwise — and for high-contrast themes — it is
 * picked as the fixed theme (`SetTheme`).
 */
export function themeChangeFor(
  settings: AppearanceSettings,
  theme: Pick<ThemeInfo, "id" | "kind">,
): { kind: "set-theme"; themeId: string } | { kind: "pair"; half: "light" | "dark" } {
  if (settings.follow_system && theme.kind !== "high-contrast") {
    return { kind: "pair", half: theme.kind };
  }
  return { kind: "set-theme", themeId: theme.id };
}
