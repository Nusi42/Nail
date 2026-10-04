/**
 * The appearance rules the UI needs outside the mock core: the built-in
 * theme ids, the override ranges and their validation, and which theme is
 * active. They mirror `nail_api::appearance`; the mock core
 * (`./mock.ts`) answers the requests with them. Kept apart from the mock so
 * the app bundle does not carry the mock core.
 */
import type {
  AppearanceSettings,
  SystemAppearance,
  ThemeInfo,
  ThemeKind,
  ThemeOverrides,
  ThemeOverridesIssue,
} from "../../api/generated";

/** `nail_api::appearance::BUILTIN_THEMES` (gallery order). */
export const BUILTIN_THEMES: readonly (readonly [string, ThemeKind])[] = [
  ["frost", "light"],
  ["sage", "light"],
  ["graphite", "dark"],
  ["nocturne", "dark"],
  ["high-contrast", "high-contrast"],
];

/** `DEFAULT_LIGHT_THEME`, `DEFAULT_DARK_THEME`, `HIGH_CONTRAST_THEME`. */
export const DEFAULT_LIGHT_THEME = "frost";
export const DEFAULT_DARK_THEME = "graphite";
export const HIGH_CONTRAST_THEME = "high-contrast";

/** `FONT_SIZE_RANGE`, `RADIUS_RANGE` (px, inclusive). */
export const FONT_SIZE_RANGE = [11, 20] as const;
export const RADIUS_RANGE = [0, 20] as const;

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
