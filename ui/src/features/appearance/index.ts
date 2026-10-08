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
 *
 * M5d WP183 (ADR 0131): the theme editor (widget and window `theme-editor`:
 * token groups, part overrides, live preview, duplicate / rename / delete),
 * the windows `theme-new`, `theme-rename` and the confirmation
 * `theme-delete`, "New theme" / "Edit theme" and the live overrides on the
 * Appearance page, Undo / Redo of the `appearance` history, the
 * inspector's "Edit style" (`./EditStyle.tsx`), the actions `theme.edit` /
 * `theme.new`, and the skin `APPEARANCE_SKIN_CSS` (`./skin.ts`). The mock
 * core answers the theme requests in `./themeEditMock.ts`.
 */
import type { ActionInfo } from "../../actions/palette";
import type { Feature } from "../types";
import { AppearanceSettings } from "./AppearanceSettings";
import { invalidateThemeDetails } from "./editorQueries";
import {
  THEME_DELETE_DIALOG,
  THEME_EDITOR_DIALOG,
  THEME_NEW_DIALOG,
  THEME_RENAME_DIALOG,
  openNewTheme,
  openThemeEditor,
} from "./editorStore";
import { DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME } from "./model";
import { appearanceKeys, applyAppearance, preferencesOf } from "./queries";
import type { Appearance } from "../../api/generated";
import { SystemAppearanceReporter } from "./SystemAppearance";
import { DeleteThemeDialog, NewThemeDialog, RenameThemeDialog } from "./ThemeDialogs";
import { ThemeEditorDialog, ThemeEditorWidget } from "./ThemeEditor";

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
  APPEARANCE_DIALOGS,
  THEME_DELETE_DIALOG,
  THEME_EDITOR_DIALOG,
  THEME_NEW_DIALOG,
  THEME_RENAME_DIALOG,
  openNewTheme,
  openThemeEditor,
  themeEditorStoreFor,
} from "./editorStore";
export { EditStyleButton } from "./EditStyle";
export { APPEARANCE_SKIN_CSS } from "./skin";
export { ThemeEditorView } from "./ThemeEditor";
export {
  appearanceKeys,
  applyAppearance,
  preferencesOf,
  useAppearance,
  useSetTheme,
  useSetThemePreferences,
} from "./queries";

/** The theme editor's actions. */
export const THEME_EDIT_ACTION = "theme.edit";
export const THEME_NEW_ACTION = "theme.new";
/**
 * Back to the built-in look (ADR 0138): Follow Windows with the default
 * pair and no overrides; the density and the theme files stay. A way out
 * of an unreadable theme that needs only Ctrl+K and typing; undoable.
 */
export const THEME_RESET_ACTION = "theme.reset";

/**
 * Layout widget kinds → components (`appearance-settings`: Settings →
 * Appearance; `theme-editor`: the theme editor, placed anywhere).
 */
export const widgets: Feature["widgets"] = {
  "appearance-settings": AppearanceSettings,
  [THEME_EDITOR_DIALOG]: ThemeEditorWidget,
};
/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  /** Opens the theme editor on the active theme. */
  [THEME_EDIT_ACTION]: {
    run: (ctx) => {
      openThemeEditor(ctx.store, null);
    },
  },
  /** Opens "New theme" (starting from the active theme). */
  [THEME_NEW_ACTION]: {
    run: (ctx) => {
      openNewTheme(ctx.store);
    },
  },
  /** Built-in themes, Follow Windows, no overrides. */
  [THEME_RESET_ACTION]: {
    run: async (ctx) => {
      const current = (await ctx.client.request({ type: "ListThemes" }, "Themes")).appearance;
      const answer = await ctx.client.request(
        {
          type: "SetThemePreferences",
          preferences: {
            ...preferencesOf(current),
            follow_system: true,
            light: DEFAULT_LIGHT_THEME,
            dark: DEFAULT_DARK_THEME,
            overrides: {},
          },
        },
        "Themes",
      );
      ctx.queryClient.setQueryData<Appearance>(appearanceKeys.themes, answer.appearance);
    },
  },
};
/** Command-palette descriptions (with their icons). */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
  [THEME_EDIT_ACTION]: {
    title: "appearance-action-edit-theme",
    keywords: "appearance-action-edit-theme-keywords",
    icon: "palette",
  },
  [THEME_NEW_ACTION]: {
    title: "appearance-action-new-theme",
    keywords: "appearance-action-new-theme-keywords",
    icon: "plus",
  },
  [THEME_RESET_ACTION]: {
    title: "appearance-action-reset",
    keywords: "appearance-action-reset-keywords",
    icon: "rotate-ccw",
  },
};
/** Dialog names → components. */
export const dialogs: NonNullable<Feature["dialogs"]> = {
  [THEME_EDITOR_DIALOG]: ThemeEditorDialog,
  [THEME_NEW_DIALOG]: NewThemeDialog,
  [THEME_RENAME_DIALOG]: RenameThemeDialog,
  [THEME_DELETE_DIALOG]: DeleteThemeDialog,
};
/**
 * The window model (M5c): the editor is a large tool window, New / Rename
 * small ones, "Delete theme?" a confirmation.
 */
export const windows: NonNullable<Feature["windows"]> = {
  [THEME_EDITOR_DIALOG]: {
    size: { width: 1100, height: 760 },
    min_size: { width: 560, height: 420 },
    pop_out: false,
  },
  [THEME_NEW_DIALOG]: {
    size: { width: 480, height: 400 },
    min_size: { width: 360, height: 320 },
    pop_out: false,
  },
  [THEME_RENAME_DIALOG]: {
    size: { width: 460, height: 320 },
    min_size: { width: 360, height: 260 },
    pop_out: false,
  },
  [THEME_DELETE_DIALOG]: { confirmation: true },
};
/**
 * Keeps the theme list current when the core reports a change, and fetches
 * the resolved theme again so the switch shows at once (the core also sends
 * `ConfigChanged` when the resolved theme changed; the refetch covers a
 * core or a remote session that only sent `ThemeChanged`).
 */
export const events: NonNullable<Feature["events"]> = [
  (event, ctx) => {
    if (event.type === "ThemeChanged") applyAppearance(ctx.queryClient, event.appearance);
    // Undo, redo or a change on disk: the editor shows the files as they are now.
    if (event.type === "EditHistoryChanged" && event.history.scope === "appearance") {
      void invalidateThemeDetails(ctx.queryClient);
    }
  },
];
/** Background components: Follow Windows from the browser / WebView. */
export const effects: NonNullable<Feature["effects"]> = [SystemAppearanceReporter];

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
      "appearance-font-size",
      "appearance-radius",
      "appearance-new-theme",
      "appearance-edit-theme",
    ],
    keywords: "settings-keywords-appearance-settings",
  },
  [THEME_EDITOR_DIALOG]: {
    labels: [
      "theme-editor-title",
      "theme-editor-group-color",
      "theme-editor-group-type",
      "theme-editor-group-spacing",
      "theme-editor-group-radius",
      "theme-editor-group-motion",
      "theme-editor-parts",
    ],
    keywords: "appearance-action-edit-theme-keywords",
  },
};
