/**
 * Feature slot of M1b WP22 (rules): filter rules: rule list, visual and text editor, test-rule preview, run history with undo.
 *
 * Only WP22 edits the files in this directory. The central registry
 * (`../registry.ts`) merges these exports into the app; see `../README.md`.
 */
import type { ActionContext } from "../../actions/registry";
import { findSummary } from "../../state/mailOps";
import type { Feature } from "../types";
import { onRulesEvent } from "./api";
import { ruleFromMessage } from "./model";
import { RuleManager, RulesDialog } from "./RuleManager";
import { rulesStore } from "./store";

/** The window (or, without one in the layout, the dialog) with the rules. */
export const RULES_WINDOW = "rules";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "rule-manager": RuleManager,
};

function selectedSummary(ctx: ActionContext) {
  const id = ctx.store.getState().selectedMessageId;
  return id === null ? undefined : findSummary(ctx.queryClient, id);
}

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  /** Opens the rules window. */
  "rules.open": {
    run: (ctx) => {
      ctx.store.getState().openWindow(RULES_WINDOW);
    },
  },
  /** "Create rule from message": a new rule pre-filled from the selected message. */
  "rules.create-from-message": {
    run: (ctx) => {
      const message = selectedSummary(ctx);
      if (!message) return;
      rulesStore(ctx.client).getState().startNew(ruleFromMessage(message));
      ctx.store.getState().openWindow(RULES_WINDOW);
    },
    enabled: (ctx) => selectedSummary(ctx) !== undefined,
  },
};

/** Named dialogs (opened with `window.open "<name>"`). */
export const dialogs: Feature["dialogs"] = {
  [RULES_WINDOW]: RulesDialog,
};

/**
 * The window model of this feature's windows (M5c, ADR 0116): tool windows
 * are non-modal with their default and minimum sizes; only confirmations
 * (`confirmation: true`) are modal. The layout's `window` properties win.
 */
export const windows: NonNullable<Feature["windows"]> = {
  [RULES_WINDOW]: { size: { width: 900, height: 640 }, min_size: { width: 600, height: 420 } },
};

/** Handlers for core events. */
export const events: Feature["events"] = [
  (event, { queryClient }) => {
    onRulesEvent(event, queryClient);
  },
];

/** Background components mounted once in the app shell. */
export const effects: Feature["effects"] = [];

/**
 * Settings labels for the settings search (M5b review: the local rules are
 * part of Settings → Mail & junk).
 */
export const settings: NonNullable<Feature["settings"]> = {
  "rule-manager": {
    labels: ["rules-title", "rules-new", "rules-list-label"],
  },
};
