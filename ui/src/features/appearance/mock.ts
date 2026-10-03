/**
 * The mock core's answers to the M5a appearance requests (contract ADR
 * 0102): the built-in themes, the selection (fixed theme or Follow Windows
 * with a light / dark pair), density and overrides, validated like the core
 * (`nail_api::appearance`). `../../api/mock.ts` routes exactly these
 * requests here; `setMockSystemAppearance` plays Windows switching modes.
 */
import type {
  Appearance,
  AppearanceSettings,
  Request,
  ResolvedTheme,
  Response,
  SystemAppearance,
  ThemeInfo,
  ThemeKind,
  ThemeOverrides,
  ThemeOverridesIssue,
} from "../../api/generated";
import type { MockCore } from "../../api/mockMail";
import { defaultTheme } from "../../api/mockData";
import { BUILTIN_THEME_FILES, resolveMockTheme } from "./themes";

/** The requests this feature answers. */
export const APPEARANCE_REQUESTS = [
  "ListThemes",
  "SetTheme",
  "SetThemePreferences",
  "ReportSystemAppearance",
] as const satisfies readonly Request["type"][];

export type AppearanceRequest = Extract<Request, { type: (typeof APPEARANCE_REQUESTS)[number] }>;

export function isAppearanceRequest(request: Request): request is AppearanceRequest {
  return (APPEARANCE_REQUESTS as readonly string[]).includes(request.type);
}

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

/** The built-in themes as the core lists them. */
export function builtinThemes(): ThemeInfo[] {
  return BUILTIN_THEMES.map(([id, kind]) => ({
    id,
    name: id,
    label: `theme-name-${id}`,
    kind,
    source: "builtin",
    extends: "default",
    file: `defaults/themes/${id}.kdl`,
    preview: previewOf(id),
  }));
}

/** `ThemeInfo.preview` of a built-in: its own colours of `PREVIEW_TOKENS`. */
function previewOf(id: string): Record<string, string> {
  const colors = BUILTIN_THEME_FILES.get(id)?.tokens.color ?? {};
  return Object.fromEntries(
    PREVIEW_TOKENS.flatMap((token) => {
      const value = colors[token];
      return value === undefined ? [] : [[token, value] as const];
    }),
  );
}

/** `nail_api::appearance::PREVIEW_TOKENS`. */
export const PREVIEW_TOKENS = ["bg-subtle", "bg", "accent", "fg"] as const;

/** `AppearanceSettings::default()`: Follow Windows with Frost / Graphite. */
export function defaultSettings(): AppearanceSettings {
  return {
    theme: DEFAULT_LIGHT_THEME,
    follow_system: true,
    light: DEFAULT_LIGHT_THEME,
    dark: DEFAULT_DARK_THEME,
    density: "comfortable",
    overrides: {},
  };
}

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

/** Mock-core state of this feature. */
export interface AppearanceMockState {
  themes: ThemeInfo[];
  settings: AppearanceSettings;
  system: SystemAppearance | undefined;
}

export function createAppearanceMockState(): AppearanceMockState {
  return {
    themes: builtinThemes(),
    settings: defaultSettings(),
    system: { scheme: "light", high_contrast: false },
  };
}

function snapshot(state: AppearanceMockState): Appearance {
  const appearance: Appearance = {
    themes: structuredClone(state.themes),
    settings: structuredClone(state.settings),
    active: activeTheme(state.settings, state.system, state.themes),
  };
  if (state.system) appearance.system = { ...state.system };
  return appearance;
}

function invalid(message: string, id: string): Response {
  return { type: "Error", code: "invalid_request", message, localized: { id } };
}

let pristine: ResolvedTheme | null = null;

/**
 * The resolved theme `GetConfig` answers: the user's theme (`data.theme`)
 * with the active built-in theme under it, density and overrides applied
 * (`./themes.ts`). Tokens the user changed (they differ from the shipped
 * default) stay the user's, like in the core where the user file wins.
 */
export function mockConfigTheme(core: MockCore): ResolvedTheme {
  const state = core.data.features.appearance;
  pristine ??= defaultTheme();
  const active = activeTheme(state.settings, state.system, state.themes);
  return resolveMockTheme(core.data.theme, active, state.settings, pristine);
}

function changed(core: MockCore, state: AppearanceMockState, before: string | null): Response {
  const appearance = snapshot(state);
  core.emit({ type: "ThemeChanged", appearance: structuredClone(appearance) });
  // Like the core: `ConfigChanged` follows when the resolved theme changed.
  const theme = mockConfigTheme(core);
  if (before === null || JSON.stringify(theme) !== before) {
    core.emit({ type: "ConfigChanged", theme, layout: structuredClone(core.data.layout) });
  }
  return { type: "Themes", appearance };
}

/** The resolved theme before a change (to tell whether it changed). */
function themeKey(core: MockCore): string {
  return JSON.stringify(mockConfigTheme(core));
}

/** Windows switched light / dark or contrast (Follow Windows scenarios). */
export function setMockSystemAppearance(core: MockCore, system: SystemAppearance): void {
  const state = core.data.features.appearance;
  const before = themeKey(core);
  state.system = { ...system };
  changed(core, state, before);
}

export function answerAppearance(core: MockCore, request: AppearanceRequest): Response {
  const state = core.data.features.appearance;
  const usable = (id: string, kinds: readonly ThemeKind[]) =>
    state.themes.some((t) => t.id === id && t.problem === undefined && kinds.includes(t.kind));
  switch (request.type) {
    case "ListThemes":
      return { type: "Themes", appearance: snapshot(state) };
    case "ReportSystemAppearance": {
      // The mock has no OS source of its own: the page's report applies
      // (like a core without the Windows watcher).
      const now = state.system;
      const next = request.system;
      if (now?.scheme !== next.scheme || now.high_contrast !== next.high_contrast) {
        setMockSystemAppearance(core, next);
      }
      return { type: "Themes", appearance: snapshot(state) };
    }
    case "SetTheme": {
      if (!usable(request.theme_id, ["light", "dark", "high-contrast"])) {
        return invalid(`no usable theme ${request.theme_id}`, "theme-problem-unknown");
      }
      const before = themeKey(core);
      state.settings = { ...state.settings, theme: request.theme_id, follow_system: false };
      return changed(core, state, before);
    }
    case "SetThemePreferences": {
      const p = request.preferences;
      if (!usable(p.light, ["light"]) || !usable(p.dark, ["dark"])) {
        return invalid("the pair needs a light and a dark theme", "theme-problem-unknown");
      }
      const issue = overrideIssues(p.overrides)[0];
      if (issue) return invalid(issue, `theme-overrides-issue-${issue}`);
      const before = themeKey(core);
      state.settings = {
        ...state.settings,
        follow_system: p.follow_system,
        light: p.light,
        dark: p.dark,
        density: p.density,
        overrides: structuredClone(p.overrides),
      };
      return changed(core, state, before);
    }
  }
}
