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
} from "../../api/generated";
import type { MockCore } from "../../api/mockMail";
import { recordMockEdit, registerMockHistoryScope } from "../../api/editHistoryMock";
import { defaultTheme } from "../../api/mockData";
import {
  BUILTIN_THEMES,
  DEFAULT_DARK_THEME,
  DEFAULT_LIGHT_THEME,
  activeTheme,
  overrideIssues,
} from "./model";
import { BUILTIN_THEME_FILES, resolveMockTheme } from "./themes";

export {
  BUILTIN_THEMES,
  DEFAULT_DARK_THEME,
  DEFAULT_LIGHT_THEME,
  FONT_SIZE_RANGE,
  HIGH_CONTRAST_THEME,
  RADIUS_RANGE,
  activeTheme,
  isHexColor,
  overrideIssues,
} from "./model";

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

/**
 * What the `appearance` undo history covers in the mock (M5d, ADR 0131):
 * the selection and overrides, the theme list and the user's theme.
 */
interface AppearanceSnapshot {
  settings: AppearanceMockState["settings"];
  themes: AppearanceMockState["themes"];
  theme: ResolvedTheme;
}

function appearanceSnapshot(core: MockCore): AppearanceSnapshot {
  const state = core.data.features.appearance;
  return structuredClone({
    settings: state.settings,
    themes: state.themes,
    theme: core.data.theme,
  });
}

registerMockHistoryScope("appearance", {
  snapshot: appearanceSnapshot,
  restore: (core, raw) => {
    const snapshot = raw as AppearanceSnapshot;
    const state = core.data.features.appearance;
    const before = themeKey(core);
    state.settings = snapshot.settings;
    state.themes = snapshot.themes;
    core.data.theme = snapshot.theme;
    changed(core, state, before);
  },
});

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
      const undo = appearanceSnapshot(core);
      state.settings = { ...state.settings, theme: request.theme_id, follow_system: false };
      const answer = changed(core, state, before);
      recordMockEdit(core, "appearance", "set-theme", undo);
      return answer;
    }
    case "SetThemePreferences": {
      const p = request.preferences;
      if (!usable(p.light, ["light"]) || !usable(p.dark, ["dark"])) {
        return invalid("the pair needs a light and a dark theme", "theme-problem-unknown");
      }
      const issue = overrideIssues(p.overrides)[0];
      if (issue) return invalid(issue, `theme-overrides-issue-${issue}`);
      const before = themeKey(core);
      const undo = appearanceSnapshot(core);
      state.settings = {
        ...state.settings,
        follow_system: p.follow_system,
        light: p.light,
        dark: p.dark,
        density: p.density,
        overrides: structuredClone(p.overrides),
      };
      const answer = changed(core, state, before);
      recordMockEdit(core, "appearance", "set-theme-preferences", undo);
      return answer;
    }
  }
}
