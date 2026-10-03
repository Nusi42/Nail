/**
 * Feature slot of M5b WP163 (contract ADR 0109): Settings as a full page —
 * the `settings-page` widget renders the layout's `settings-page` section
 * (`ResolvedLayout.settings`: groups General / Look & feel / Security /
 * Extensions → sections → widget trees) with grouped navigation, a search
 * over every section, group and setting label with highlighted matches and
 * deep links, and each widget of the shown section on a card. The search
 * index is built by `../../layout/settingsIndex.ts` from the page and the
 * labels each feature registers per settings widget kind (`Feature.settings`,
 * merged as `appRegistry.settings`); every feature folder except `pim/`
 * registers its own, the built-in widgets' are in `builtins.ts`. The shown
 * section is remembered in the core's UI state (`settings.section`,
 * `../../state/uiState.ts`). Texts: `locales/{en,de}/settings.ftl` (group
 * and section labels of the default layout in `core-shell.ftl`); styles:
 * `skin.ts` (`SETTINGS_SKIN_CSS`).
 *
 * Actions: `settings.section "<id>"` opens the page at a section — the id
 * may also be a node id on the page, a widget kind or the id of a former
 * settings tab (`FORMER_SETTINGS_TABS`), which then reveals that widget;
 * `settings.search` opens the page with the search focused.
 */
import type { ActionInfo } from "../../actions/palette";
import type { ActionContext } from "../../actions/registry";
import type { SettingsWidgetInfo } from "../../layout/settingsIndex";
import { SETTINGS_SEARCH_ACTION, SETTINGS_SECTION_ACTION, viewDef } from "../../layout/views";
import { UI_STATE_SETTINGS_SECTION, applyUiStateChange } from "../../state/uiState";
import { showView } from "../shell";
import type { Feature } from "../types";
import { BUILTIN_SETTINGS } from "./builtins";
import { resolveSettingsTarget } from "./model";
import { settingsStoreFor } from "./store";
import { SettingsPage } from "./widgets";

export { FORMER_SETTINGS_TABS, highlightText, resolveSettingsTarget } from "./model";
export { settingsStoreFor } from "./store";
export { SETTINGS_SKIN_CSS } from "./skin";

/** The window the settings page lives in when the layout has no `settings` view. */
const SETTINGS_WINDOW = "settings";
/** The view that shows the settings page. */
const SETTINGS_VIEW = "settings";

/** Shows the settings page: its view (rail / view host), else its window. */
export function openSettingsPage(ctx: ActionContext): void {
  if (viewDef(ctx.layout, SETTINGS_VIEW) !== null) showView(ctx, SETTINGS_VIEW);
  else ctx.store.getState().openWindow(SETTINGS_WINDOW);
}

/** Remembers the shown section (the cache at once, then the core). */
function rememberSection(ctx: ActionContext, section: string): void {
  applyUiStateChange(ctx.queryClient, UI_STATE_SETTINGS_SECTION, section);
  void ctx.client
    .call({ type: "SetUiState", key: UI_STATE_SETTINGS_SECTION, value: section })
    .catch(() => undefined);
}

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "settings-page": SettingsPage,
};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  [SETTINGS_SECTION_ACTION]: {
    run: (ctx, args) => {
      const id = args[0];
      if (typeof id !== "string") return;
      const target = resolveSettingsTarget(ctx.layout, id);
      if (target === null) return;
      rememberSection(ctx, target.section);
      if (target.node !== null || target.widget !== null) {
        settingsStoreFor(ctx.store).getState().requestReveal({
          section: target.section,
          node: target.node,
          widget: target.widget,
          text: null,
        });
      }
      openSettingsPage(ctx);
    },
    enabled: (ctx, args) =>
      typeof args[0] === "string" && resolveSettingsTarget(ctx.layout, args[0]) !== null,
  },
  [SETTINGS_SEARCH_ACTION]: {
    run: (ctx) => {
      settingsStoreFor(ctx.store).getState().requestSearchFocus();
      openSettingsPage(ctx);
    },
    enabled: (ctx) => ctx.layout.settings.length > 0,
  },
};

/** Command-palette descriptions. */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
  // Needs a section argument: not listed.
  [SETTINGS_SECTION_ACTION]: { title: "settings-section-action", hidden: true, icon: "settings" },
  [SETTINGS_SEARCH_ACTION]: {
    title: "settings-search-action",
    keywords: "settings-keywords-search",
    icon: "search",
  },
};

/** Settings labels per widget kind: the built-in widgets (`builtins.ts`). */
export const settings: Readonly<Record<string, SettingsWidgetInfo>> = BUILTIN_SETTINGS;
/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};
/** Handlers for core events. */
export const events: Feature["events"] = [];
/** Background components. */
export const effects: Feature["effects"] = [];
