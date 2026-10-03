/**
 * Feature slot of M5b WP163 (contract ADR 0109): Settings as a full page —
 * the `settings-page` widget renders the layout's `settings-page` section
 * (`ResolvedLayout.settings`: groups General / Look & feel / Security /
 * Extensions → sections → widget trees) with grouped navigation, a search
 * over every setting label, and the section shown. The search index is
 * built by `../../layout/settingsIndex.ts` from the page and the labels
 * each feature registers per settings widget kind (`Feature.settings`,
 * merged as `appRegistry.settings`); WP163 adds those registrations to
 * every feature folder except `pim/`, and keeps the built-in widgets'
 * (`account-settings`, `virtual-account-editor`, `toolbar`, …) here. The
 * last section is remembered in the core's UI state (`settings.section`,
 * `../../state/uiState.ts`). Texts: `locales/{en,de}/settings.ftl` (group
 * and section labels of the default layout in `core-shell.ftl`).
 *
 * Contract stage: `settings.section "<id>"` opens the settings window (the
 * page lives there once WP165 swaps the tabs for it) and remembers the
 * section; the widget renders nothing yet.
 */
import type { ActionInfo } from "../../actions/palette";
import type { SettingsWidgetInfo } from "../../layout/settingsIndex";
import { SETTINGS_SECTION_ACTION } from "../../layout/views";
import { UI_STATE_SETTINGS_SECTION } from "../../state/uiState";
import type { Feature } from "../types";
import { SettingsPage } from "./widgets";

/** The window the settings page lives in (the `settings` view's window). */
const SETTINGS_WINDOW = "settings";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "settings-page": SettingsPage,
};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  [SETTINGS_SECTION_ACTION]: {
    run: (ctx, args) => {
      const section = args[0];
      if (typeof section !== "string") return;
      void ctx.client
        .call({ type: "SetUiState", key: UI_STATE_SETTINGS_SECTION, value: section })
        .catch(() => undefined);
      const window = ctx.layout.views.settings?.window ?? SETTINGS_WINDOW;
      ctx.store.getState().openWindow(window);
    },
    enabled: (ctx, args) =>
      typeof args[0] === "string" &&
      ctx.layout.settings.some((g) => g.sections.some((s) => s.id === args[0])),
  },
};

/** Command-palette descriptions (needs a section argument: not listed). */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
  [SETTINGS_SECTION_ACTION]: { title: "settings-section-action", hidden: true, icon: "settings" },
};

/** Settings labels per widget kind (WP163 fills these in every feature). */
export const settings: Readonly<Record<string, SettingsWidgetInfo>> = {};
/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};
/** Handlers for core events. */
export const events: Feature["events"] = [];
/** Background components. */
export const effects: Feature["effects"] = [];
