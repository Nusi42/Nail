/**
 * "Run rule" in the message-list row menu and the folder-tree folder menu
 * (R1, ADR 0219).
 *
 * - `rules.run-menu` (the layout's menu item) replaces the open context
 *   menu, in place, with a menu of the saved rules: "All rules that are on"
 *   (when there are several), the rules that are on, then under "Off" the
 *   rules that are off (running a rule by hand works for those too).
 *   Without rules it offers to open the rules window. The menu keeps the
 *   target of the menu it came from.
 * - `rules.run` (argument: a rule id, or {@link ALL_RULES}) runs on that
 *   target: the selected rows of the list (a conversation row stands for its
 *   messages in the listed folder) or the folder of the folder menu.
 * - What the run did shows in `RunToast`, with Undo for the whole click.
 *
 * The rules come from the layout-independent `ListRules` (they are the
 * user's data), so the menu is built at run time rather than in `layout.kdl`.
 */
import type { QueryClient } from "@tanstack/react-query";
import type { ActionContext } from "../../actions/registry";
import { currentTargets } from "../../actions/mailTargets";
import { errorMessageIdOf } from "../../api/client";
import type { FolderId, MessageTarget, ResolvedMenuEntry, Rule, RuleId } from "../../api/generated";
import type { UiStore } from "../../state/uiStore";
import { contextTarget, menuStoreFor } from "../menus";
import type { Feature } from "../types";
import { rulesKeys, rulesQuery, runRules, type RuleRunTarget } from "./api";
import { messageIdsOf } from "./runTargets";
import { rulesStore } from "./store";

export const RUN_MENU_ACTION = "rules.run-menu";
export const RUN_ACTION = "rules.run";
/** The `rules.run` argument for every rule that is on. */
export const ALL_RULES = "*";
/** Name of the rules menu (it replaces the menu it was opened from). */
export const RUN_MENU_NAME = "rules:run";

/** Opens the rules window (also "No rules yet" of the rules menu). */
export const OPEN_RULES_ACTION = "rules.open";

/** What a menu's run acts on. */
export type MenuRunTarget =
  { kind: "folder"; folderId: FolderId } | { kind: "messages"; targets: MessageTarget[] };

/** The target of the open menu: its folder, or the list rows it stands for. */
export function menuRunTarget(ui: UiStore): MenuRunTarget | null {
  const target = contextTarget(ui);
  if (target?.part === "folder-tree.folder" && target.folderId !== null) {
    return { kind: "folder", folderId: target.folderId };
  }
  if (target?.part === "message-list.row") {
    const { targets } = currentTargets(ui);
    return targets.length > 0 ? { kind: "messages", targets } : null;
  }
  return null;
}

function runItem(arg: string, label: string): ResolvedMenuEntry {
  return { type: "action", action: RUN_ACTION, args: [arg], label, icon: null, shortcut: null };
}

/** The entries of the rules menu. */
export function runMenuItems(rules: readonly Rule[]): ResolvedMenuEntry[] {
  if (rules.length === 0) {
    return [
      {
        type: "action",
        action: OPEN_RULES_ACTION,
        args: [],
        label: "@rules.menu.none",
        icon: null,
        shortcut: null,
      },
    ];
  }
  const on = rules.filter((r) => r.enabled);
  const off = rules.filter((r) => !r.enabled);
  const out: ResolvedMenuEntry[] = [];
  if (on.length > 1) out.push(runItem(ALL_RULES, "@rules.menu.all"), { type: "separator" });
  out.push(...on.map((r) => runItem(r.id, r.name)));
  if (off.length > 0) {
    if (on.length > 0) out.push({ type: "separator" });
    out.push(
      { type: "header", label: "@rules.menu.off" },
      ...off.map((r) => runItem(r.id, r.name)),
    );
  }
  return out;
}

async function savedRules(ctx: ActionContext): Promise<Rule[]> {
  return ctx.queryClient.query(rulesQuery(ctx.client));
}

/** The rule ids a `rules.run` argument names. */
export function ruleIdsOf(arg: string, rules: readonly Rule[]): RuleId[] {
  return arg === ALL_RULES ? rules.filter((r) => r.enabled).map((r) => r.id) : [arg];
}

async function runTargetOfMenu(ctx: ActionContext, target: MenuRunTarget): Promise<RuleRunTarget> {
  return target.kind === "folder"
    ? { messageIds: [], folderId: target.folderId }
    : { messageIds: await messageIdsOf(ctx.client, target.targets), folderId: null };
}

/** Refetches the run histories after runs or undos. */
export function refreshRuns(queryClient: QueryClient): Promise<void> {
  return queryClient.invalidateQueries({ queryKey: rulesKeys.allRuns });
}

export const runMenuActions: Feature["actions"] = {
  [RUN_MENU_ACTION]: {
    run: async (ctx) => {
      // Read before the menu closes (the host closes it after this returns).
      const menus = menuStoreFor(ctx.store);
      const open = menus.getState().open;
      if (!open) return;
      try {
        const rules = await savedRules(ctx);
        menus
          .getState()
          .show({ ...open, menu: { name: RUN_MENU_NAME, items: runMenuItems(rules) } });
      } catch (error) {
        rulesStore(ctx.client)
          .getState()
          .showToast({ kind: "error", id: errorMessageIdOf(error) });
      }
    },
    enabled: (ctx) => menuRunTarget(ctx.store) !== null,
  },
  [RUN_ACTION]: {
    run: async (ctx, args) => {
      const target = menuRunTarget(ctx.store);
      const [arg] = args;
      if (!target || typeof arg !== "string") return;
      const toast = rulesStore(ctx.client).getState();
      try {
        const ids = arg === ALL_RULES ? ruleIdsOf(arg, await savedRules(ctx)) : [arg];
        const runs = await runRules(ctx.client, ids, await runTargetOfMenu(ctx, target));
        toast.showToast({ kind: "ran", runs });
        await refreshRuns(ctx.queryClient);
      } catch (error) {
        toast.showToast({ kind: "error", id: errorMessageIdOf(error) });
      }
    },
    enabled: (ctx, args) => typeof args[0] === "string" && menuRunTarget(ctx.store) !== null,
  },
};
