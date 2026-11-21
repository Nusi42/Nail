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
import {
  fromMessageActionInfo,
  fromMessageActions,
  fromMessageMenuSources,
  selectedOrigin,
} from "./fromMessage";
import { ruleFromMessage } from "./model";
import { RuleManager, RulesDialog } from "./RuleManager";
import {
  OPEN_RULES_ACTION,
  RUN_ACTION,
  RUN_MENU_ACTION,
  runMenuActions,
  runMenuSource,
} from "./runMenu";
import { RULES_WINDOW, RulesCloseGuard } from "./closeGuard";
import { RulesPopOutPreparer } from "./popOut";
import { RunToast } from "./RunToast";
import { rulesStore } from "./store";

/** The window (or, without one in the layout, the dialog) with the rules. */
export { RULES_WINDOW };

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "rule-manager": RuleManager,
};

function selectedSummary(ctx: ActionContext) {
  const origin = selectedOrigin(ctx);
  return origin === null ? undefined : findSummary(ctx.queryClient, origin.messageId);
}

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  /** Opens the rules window. */
  [OPEN_RULES_ACTION]: {
    run: (ctx) => {
      rulesStore(ctx.client).getState().setOrigin(null);
      ctx.store.getState().openWindow(RULES_WINDOW);
    },
  },
  /**
   * "Create rule from message": a new rule pre-filled from the selected
   * message; the window remembers the message (R2: values from this mail).
   */
  "rules.create-from-message": {
    run: (ctx) => {
      const origin = selectedOrigin(ctx);
      const message = selectedSummary(ctx);
      if (!origin || !message) return;
      const rules = rulesStore(ctx.client).getState();
      rules.setOrigin(origin);
      rules.startNew(ruleFromMessage(message));
      ctx.store.getState().openWindow(RULES_WINDOW);
    },
    enabled: (ctx) => selectedOrigin(ctx) !== null,
  },
  /** "Run rule" (menus) and running one rule, or every rule that is on, from it. */
  ...runMenuActions,
  /** "Add to rule…" (message menus) and its rules: the window with the mail as origin. */
  ...fromMessageActions(RULES_WINDOW),
};

/** "Run rule" and "Add to rule…": submenus of the saved rules (R2, ADR 0222). */
export const menuSources: NonNullable<Feature["menuSources"]> = {
  [RUN_MENU_ACTION]: runMenuSource,
  ...fromMessageMenuSources,
};

/**
 * Palette descriptions: the run actions only make sense in their menus
 * (hidden); their icon is described here, the layout names only the item.
 */
export const actionInfo: NonNullable<Feature["actionInfo"]> = {
  [RUN_MENU_ACTION]: { title: "rules-action-run-menu", hidden: true, icon: "list-checks" },
  [RUN_ACTION]: { title: "rules-action-run-menu", hidden: true, icon: "list-checks" },
  ...fromMessageActionInfo,
};

/**
 * Background components: the toast of a run from a menu, the guard that
 * keeps the rules window open while its rule has unsaved changes (R2), and
 * the preparer that hands a popped-out rules window the mail and rule.
 */
export const effects: NonNullable<Feature["effects"]> = [
  RunToast,
  RulesCloseGuard,
  RulesPopOutPreparer,
];

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
