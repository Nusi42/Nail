/**
 * Feature slot of M1b WP22 (rules): filter rules: rule list, visual and text editor, test-rule preview, run history with undo.
 * R1 (ADR 0219): running rules on demand — the run panel of the rules
 * window, "Run rule" in the message-list row and folder menus
 * (`./runMenu.ts`) and the toast of a menu's run (`./RunToast.tsx`).
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
import { OPEN_RULES_ACTION, RUN_ACTION, RUN_MENU_ACTION, runMenuActions } from "./runMenu";
import { RunToast } from "./RunToast";
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
  [OPEN_RULES_ACTION]: {
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
  /** "Run rule" (menus) and running one rule, or every rule that is on, from it. */
  ...runMenuActions,
};

/**
 * Palette descriptions: the run actions only make sense in their menus
 * (hidden); their icon is described here, the layout names only the item.
 */
export const actionInfo: NonNullable<Feature["actionInfo"]> = {
  [RUN_MENU_ACTION]: { title: "rules-action-run-menu", hidden: true, icon: "list-checks" },
  [RUN_ACTION]: { title: "rules-action-run-menu", hidden: true, icon: "list-checks" },
};

/** Background components: the toast of a run from a menu. */
export const effects: NonNullable<Feature["effects"]> = [RunToast];

/** Named dialogs (opened with `window.open "<name>"`). */
export const dialogs: NonNullable<Feature["dialogs"]> = {
  [RULES_WINDOW]: RulesDialog,
};

/** Handlers for core events. */
export const events: NonNullable<Feature["events"]> = [
  (event, { queryClient }) => {
    onRulesEvent(event, queryClient);
  },
];

/**
 * Settings labels for the settings search (M5b review: the local rules are
 * part of Settings → Mail & junk).
 */
export const settings: NonNullable<Feature["settings"]> = {
  "rule-manager": {
    labels: ["rules-title", "rules-new", "rules-list-label"],
  },
};
