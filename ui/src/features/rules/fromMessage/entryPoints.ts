/**
 * Opening the rules window from a mail (R2, ADR 0222): "Add to rule…" in
 * the message menus. Its submenu (a menu source) lists the saved rules;
 * picking one opens the rules window on that rule with the mail as the
 * window's `origin`, so the editor offers the mail's values beside every
 * condition and action field. Without the submenu (or rules) the item opens
 * the window with the mail as origin. "Create rule from message"
 * (`rules.create-from-message`) sets the origin the same way.
 *
 * The rules feature registers these (`fromMessageActions(window)`, the
 * window name is the rules feature's) beside its own actions; the layout
 * names `rules.add-to-rule` in the message menus.
 */
import type { ActionContext } from "../../../actions/registry";
import type { ResolvedMenuEntry, Rule } from "../../../api/generated";
import { findSummary } from "../../../state/mailOps";
import { literalMenuLabel, type MenuSourceRegistry } from "../../menus/sources";
import type { Feature } from "../../types";
import { rulesQuery } from "../api";
import { rulesStore, type RulesOrigin } from "../store";

/** "Add to rule…": the item with the rules submenu. */
export const ADD_TO_RULE_ACTION = "rules.add-to-rule";
/** One rule of that submenu (argument: the rule id). */
export const ADD_TO_RULE_PICK_ACTION = "rules.add-to-rule.pick";

/** The selected message as the rules window's origin, `null` without one. */
export function selectedOrigin(ctx: ActionContext): RulesOrigin | null {
  const id = ctx.store.getState().selectedMessageId;
  const message = id === null ? undefined : findSummary(ctx.queryClient, id);
  return message ? { messageId: message.id, folderId: message.folder_id } : null;
}

/** The submenu's entries: one per saved rule, named as the user wrote it. */
export function addToRuleEntries(rules: readonly Rule[]): ResolvedMenuEntry[] {
  return rules.map((rule) => ({
    type: "action",
    action: ADD_TO_RULE_PICK_ACTION,
    args: [rule.id],
    label: literalMenuLabel(rule.name),
    icon: null,
    shortcut: null,
  }));
}

/** The menu sources of this slot (the rules feature merges them). */
export const fromMessageMenuSources: MenuSourceRegistry = {
  [ADD_TO_RULE_ACTION]: {
    load: async (ctx) => addToRuleEntries(await ctx.queryClient.query(rulesQuery(ctx.client))),
  },
};

const hasOrigin = (ctx: ActionContext) => selectedOrigin(ctx) !== null;

/**
 * The actions of this slot, opening `window` (the rules window): "Add to
 * rule…" and its rules.
 */
export function fromMessageActions(window: string): NonNullable<Feature["actions"]> {
  const open = (ctx: ActionContext, rule: string | null) => {
    const origin = selectedOrigin(ctx);
    if (!origin) return;
    const rules = rulesStore(ctx.client).getState();
    rules.setOrigin(origin);
    if (rule !== null) rules.select(rule);
    ctx.store.getState().openWindow(window);
  };
  return {
    [ADD_TO_RULE_ACTION]: {
      run: (ctx) => {
        open(ctx, null);
      },
      enabled: hasOrigin,
    },
    [ADD_TO_RULE_PICK_ACTION]: {
      run: (ctx, args) => {
        const [rule] = args;
        if (typeof rule === "string") open(ctx, rule);
      },
      enabled: hasOrigin,
    },
  };
}

/**
 * Palette descriptions: "Add to rule…" belongs in the message menus; a
 * rule of its submenu only makes sense there (hidden).
 */
export const fromMessageActionInfo: NonNullable<Feature["actionInfo"]> = {
  [ADD_TO_RULE_ACTION]: { title: "rules-from-message-add-to-rule", icon: "list-plus" },
  [ADD_TO_RULE_PICK_ACTION]: {
    title: "rules-from-message-add-to-rule",
    hidden: true,
    icon: "list-plus",
  },
};
