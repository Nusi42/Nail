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
import { queryKeys } from "../../state/queries";
import type { Feature } from "../types";
import { AppearanceSettings } from "./AppearanceSettings";
import { appearanceKeys } from "./queries";
import { SystemAppearanceReporter } from "./SystemAppearance";

export {
  APPEARANCE_REQUESTS,
  BUILTIN_THEMES,
  DEFAULT_DARK_THEME,
  DEFAULT_LIGHT_THEME,
  HIGH_CONTRAST_THEME,
  activeTheme,
  isHexColor,
  overrideIssues,
  setMockSystemAppearance,
} from "./mock";
export { AppearanceSettings } from "./AppearanceSettings";
export { SystemAppearanceReporter, useBrowserAppearance } from "./SystemAppearance";
export {
  appearanceKeys,
  preferencesOf,
  useAppearance,
  useSetTheme,
  useSetThemePreferences,
} from "./queries";
export { BUILTIN_THEME_FILES, parseThemeFile, resolveMockTheme } from "./themes";

/** Layout widget kinds → components (`appearance-settings`: Settings → Appearance). */
export const widgets: Feature["widgets"] = {
  "appearance-settings": AppearanceSettings,
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
    if (event.type === "ThemeChanged") {
      const before = ctx.queryClient.getQueryData<typeof event.appearance>(appearanceKeys.themes);
      ctx.queryClient.setQueryData(appearanceKeys.themes, event.appearance);
      const switched =
        before === undefined ||
        before.active !== event.appearance.active ||
        before.settings.density !== event.appearance.settings.density ||
        JSON.stringify(before.settings.overrides) !==
          JSON.stringify(event.appearance.settings.overrides);
      if (switched) void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.config });
    }
  },
];
/** Background components: Follow Windows from the browser / WebView. */
export const effects: Feature["effects"] = [SystemAppearanceReporter];
