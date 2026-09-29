/**
 * The central registry: the built-in widgets and actions plus everything
 * the feature slots (`./<name>/index.ts`) contribute. The app shell and the
 * test harness use only this module, so a feature never edits shared files
 * to register something (see `README.md`).
 */
import { builtinActions, type ActionHandler } from "../actions/registry";
import type { DialogRegistry } from "../layout/dialogRegistry";
import type { WidgetRegistry } from "../layout/widgetRegistry";
import { builtinWidgets } from "../widgets";
import * as composer from "./composer";
import * as customization from "./customization";
import * as mcp from "./mcp";
import * as rules from "./rules";
import * as security from "./security";
import type { Feature } from "./types";

/** The feature slots, by name. */
export const features: Readonly<Record<string, Feature>> = {
  composer,
  rules,
  mcp,
  customization,
  security,
};

/** The merged registries. */
export interface AppRegistry {
  widgets: WidgetRegistry;
  actions: Readonly<Record<string, ActionHandler>>;
  dialogs: DialogRegistry;
  events: Feature["events"];
  effects: Feature["effects"];
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
 * the same widget kind, action id or dialog name.
 */
export function mergeFeatures(
  builtins: { widgets: WidgetRegistry; actions: Readonly<Record<string, ActionHandler>> },
  slots: Readonly<Record<string, Feature>>,
): AppRegistry {
  const owners = new Map<string, string>();
  const widgets: Record<string, WidgetRegistry[string]> = {};
  const actions: Record<string, ActionHandler> = {};
  const dialogs: Record<string, DialogRegistry[string]> = {};
  mergeInto(widgets, builtins.widgets, "widget", "the built-ins", owners);
  mergeInto(actions, builtins.actions, "action", "the built-ins", owners);
  const events: Feature["events"][number][] = [];
  const effects: Feature["effects"][number][] = [];
  for (const [name, feature] of Object.entries(slots)) {
    const owner = `feature "${name}"`;
    mergeInto(widgets, feature.widgets, "widget", owner, owners);
    mergeInto(actions, feature.actions, "action", owner, owners);
    mergeInto(dialogs, feature.dialogs, "dialog", owner, owners);
    events.push(...feature.events);
    effects.push(...feature.effects);
  }
  return { widgets, actions, dialogs, events, effects };
}

/** Everything the app registers. */
export const appRegistry: AppRegistry = mergeFeatures(
  { widgets: builtinWidgets, actions: builtinActions },
  features,
);
