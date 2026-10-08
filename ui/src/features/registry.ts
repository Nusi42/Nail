/**
 * The central registry: the built-in widgets and actions plus everything
 * the feature slots (`./<name>/index.ts`) contribute. The app shell and the
 * test harness use only this module, so a feature never edits shared files
 * to register something (see `README.md`).
 */
import type { ActionInfo } from "../actions/palette";
import { builtinActions, type ActionHandler } from "../actions/registry";
import type { DialogRegistry } from "../layout/dialogRegistry";
import type { WidgetRegistry } from "../layout/widgetRegistry";
import { builtinWidgets } from "../widgets";
import * as mailWidgets from "../widgets/feature";
import type { SettingsWidgetInfo } from "../layout/settingsIndex";
import type { WindowRegistration, WindowRegistry } from "../state/windows";
import type { WindowTitleFn, WindowTitleRegistry } from "../layout/windowSpecs";
import * as agents from "./agents";
import * as appearance from "./appearance";
import * as composer from "./composer";
import * as conversationList from "./conversation-list";
import * as conversationReader from "./conversation-reader";
import * as customization from "./customization";
import * as exchange from "./exchange";
import * as gmail from "./gmail";
import * as launch from "./launch";
import * as layoutEditor from "./layout-editor";
import * as mailActions from "./mail-actions";
import * as mcp from "./mcp";
import * as menus from "./menus";
import * as palette from "./palette";
import * as pim from "./pim";
import * as plugins from "./plugins";
import * as popout from "./popout";
import * as portability from "./portability";
import * as print from "./print";
import * as remote from "./remote";
import * as rules from "./rules";
import * as scheduling from "./scheduling";
import * as security from "./security";
import * as settings from "./settings";
import * as shell from "./shell";
import * as sieve from "./sieve";
import * as sync from "./sync";
import * as system from "./system";
import * as toolbarEditor from "./toolbar-editor";
import * as triage from "./triage";
import * as windows from "./windows";
import type { Feature } from "./types";

/** The feature slots, by name. */
export const features: Readonly<Record<string, Feature>> = {
  composer,
  rules,
  mcp,
  customization,
  security,
  remote,
  launch,
  sieve,
  triage,
  sync,
  menus,
  plugins,
  pim,
  scheduling,
  gmail,
  exchange,
  agents,
  system,
  portability,
  palette,
  print,
  appearance,
  // M5b (ADR 0109): the shell (WP161), the settings page (WP163) and the
  // mail widgets' own registrations (WP162, `../widgets/feature.ts`).
  shell,
  settings,
  mailWidgets,
  // M5c (ADR 0116): the window manager (WP171) and desktop pop-outs (WP173).
  windows,
  popout,
  // M5d (ADR 0131): the layout editor (WP181) and the toolbar customizer
  // (WP182); Appearance and the theme editor (WP183) live in `appearance`.
  layoutEditor,
  toolbarEditor,
  // P2 (ADR 0148): the conversation list (WP-list) and the reader's card
  // feed (WP-reader); their widgets stay message-list / thread-view.
  conversationList,
  conversationReader,
  // P3 (ADR 0164): Gmail-like conversation actions (WP-actions-ui).
  mailActions,
};

/** The merged registries. */
export interface AppRegistry {
  widgets: WidgetRegistry;
  actions: Readonly<Record<string, ActionHandler>>;
  /** Command-palette descriptions of the features' actions (M4c). */
  actionInfo: Readonly<Record<string, ActionInfo>>;
  /** Settings labels per widget kind, for the settings search (M5b). */
  settings: Readonly<Record<string, SettingsWidgetInfo>>;
  dialogs: DialogRegistry;
  /** The window model of the features' windows by kind (M5c). */
  windows: WindowRegistry;
  /** Window titles by kind from the UI state (M5c). */
  windowTitles: WindowTitleRegistry;
  events: NonNullable<Feature["events"]>;
  effects: NonNullable<Feature["effects"]>;
}

function mergeInto<T>(
  target: Record<string, T>,
  source: Readonly<Record<string, T>>,
  what: string,
  owner: string,
  owners: Map<string, string>,
): void {
  for (const [key, value] of Object.entries(source)) {
    const previous = owners.get(`${what}:${key}`);
    if (previous !== undefined) {
      throw new Error(`nail: ${what} "${key}" is registered by both ${previous} and ${owner}`);
    }
    owners.set(`${what}:${key}`, owner);
    target[key] = value;
  }
}

/**
 * Merges the built-ins and the features. Throws when two of them register
 * the same widget kind, action id, dialog name or window kind.
 */
export function mergeFeatures(
  builtins: { widgets: WidgetRegistry; actions: Readonly<Record<string, ActionHandler>> },
  slots: Readonly<Record<string, Feature>>,
): AppRegistry {
  const owners = new Map<string, string>();
  const widgets: Record<string, WidgetRegistry[string]> = {};
  const actions: Record<string, ActionHandler> = {};
  const dialogs: Record<string, DialogRegistry[string]> = {};
  const actionInfo: Record<string, ActionInfo> = {};
  const settings: Record<string, SettingsWidgetInfo> = {};
  const windowSpecs: Record<string, WindowRegistration> = {};
  const windowTitles: Record<string, WindowTitleFn> = {};
  mergeInto(widgets, builtins.widgets, "widget", "the built-ins", owners);
  mergeInto(actions, builtins.actions, "action", "the built-ins", owners);
  const events: NonNullable<Feature["events"]>[number][] = [];
  const effects: NonNullable<Feature["effects"]>[number][] = [];
  for (const [name, feature] of Object.entries(slots)) {
    const owner = `feature "${name}"`;
    mergeInto(widgets, feature.widgets, "widget", owner, owners);
    mergeInto(actions, feature.actions, "action", owner, owners);
    mergeInto(dialogs, feature.dialogs ?? {}, "dialog", owner, owners);
    mergeInto(actionInfo, feature.actionInfo ?? {}, "action info", owner, owners);
    mergeInto(settings, feature.settings ?? {}, "settings info", owner, owners);
    mergeInto(windowSpecs, feature.windows ?? {}, "window", owner, owners);
    mergeInto(windowTitles, feature.windowTitles ?? {}, "window title", owner, owners);
    events.push(...(feature.events ?? []));
    effects.push(...(feature.effects ?? []));
  }
  return {
    widgets,
    actions,
    actionInfo,
    settings,
    dialogs,
    windows: windowSpecs,
    windowTitles,
    events,
    effects,
  };
}

/** Everything the app registers. */
export const appRegistry: AppRegistry = mergeFeatures(
  { widgets: builtinWidgets, actions: builtinActions },
  features,
);
