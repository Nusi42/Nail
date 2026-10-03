/**
 * Feature slot of M5a WP153 (contract ADR 0102): appearance — switching
 * themes (built-in Frost, Sage, Graphite, Nocturne, High contrast and the
 * user's own), Follow Windows with a light / dark pair, Comfortable /
 * Compact density and the text-size / corner-radius / accent overrides,
 * through `ListThemes`, `SetTheme`, `SetThemePreferences` and the
 * `ThemeChanged` event; Lucide icons (`nail_api::icons`,
 * `crates/nail-api/fixtures/icons.json`).
 *
 * WP153 owns `ui/` in this wave (the restyled primitives live where they
 * are used); this directory holds the appearance-specific parts. The
 * central registry (`../registry.ts`) merges these exports into the app;
 * see `../README.md`. Texts: `locales/{en,de}/appearance.ftl` (contract
 * ids in `core-appearance.ftl`).
 */
import type { Feature } from "../types";

export {
  APPEARANCE_REQUESTS,
  BUILTIN_THEMES,
  DEFAULT_DARK_THEME,
  DEFAULT_LIGHT_THEME,
  HIGH_CONTRAST_THEME,
  activeTheme,
  isHexColor,
  overrideIssues,
} from "./mock";

/** Query keys of this feature. */
export const appearanceKeys = {
  all: ["appearance"] as const,
  themes: ["appearance", "themes"] as const,
};

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};
/** Action ids → handlers. */
export const actions: Feature["actions"] = {};
/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};
/** Keeps the theme list current when the core reports a change. */
export const events: Feature["events"] = [
  (event, ctx) => {
    if (event.type === "ThemeChanged") {
      ctx.queryClient.setQueryData(appearanceKeys.themes, event.appearance);
    }
  },
];
/** Background components. */
export const effects: Feature["effects"] = [];
