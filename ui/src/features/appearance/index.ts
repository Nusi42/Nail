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
import { AppearanceSettings } from "./AppearanceSettings";
import { applyAppearance } from "./queries";
import { SystemAppearanceReporter } from "./SystemAppearance";

export {
  BUILTIN_THEMES,
  DEFAULT_DARK_THEME,
  DEFAULT_LIGHT_THEME,
  HIGH_CONTRAST_THEME,
  activeTheme,
  isHexColor,
  overrideIssues,
} from "./model";
export { AppearanceSettings } from "./AppearanceSettings";
export { SystemAppearanceReporter, useBrowserAppearance } from "./SystemAppearance";
export {
  appearanceKeys,
  applyAppearance,
  preferencesOf,
  useAppearance,
  useSetTheme,
  useSetThemePreferences,
} from "./queries";

/** The full theme editor (`theme-editor`); WP183 replaces this placeholder. */
function ThemeEditorPlaceholder(): null {
  return null;
}

/** Layout widget kinds → components (`appearance-settings`: Settings → Appearance). */
export const widgets: Feature["widgets"] = {
  "appearance-settings": AppearanceSettings,
  // M5d (ADR 0131): the theme editor (WP183 replaces this placeholder).
  "theme-editor": ThemeEditorPlaceholder,
};
/** Action ids → handlers. */
export const actions: Feature["actions"] = {};
/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};
/**
 * Keeps the theme list current when the core reports a change, and fetches
 * the resolved theme again so the switch shows at once (the core also sends
 * `ConfigChanged` when the resolved theme changed; the refetch covers a
 * core or a remote session that only sent `ThemeChanged`).
 */
export const events: Feature["events"] = [
  (event, ctx) => {
    if (event.type === "ThemeChanged") applyAppearance(ctx.queryClient, event.appearance);
  },
];
/** Background components: Follow Windows from the browser / WebView. */
export const effects: Feature["effects"] = [SystemAppearanceReporter];

/**
 * Settings labels per widget kind, for the settings search (M5b WP163, ADR
 * 0109): Fluent ids of the setting labels each widget shows.
 */
export const settings: NonNullable<Feature["settings"]> = {
  "appearance-settings": {
    labels: [
      "settings-tab-appearance",
      "appearance-title",
      "appearance-themes-title",
      "appearance-gallery-label",
      "appearance-follow-system",
      "appearance-light-theme",
      "appearance-dark-theme",
      "appearance-density-title",
      "density-comfortable",
      "density-compact",
      "appearance-overrides-title",
      "appearance-accent",
    ],
    keywords: "settings-keywords-appearance-settings",
  },
};
