/**
 * "Run rule" in the message-list row menu and the folder-tree folder menu
 * (R1, ADR 0219; a hover submenu since R2, ADR 0222).
 *
 * - `rules.run-menu` (the layout's menu item) has a menu source
 *   (`runMenuSource`, `../menus/sources.ts`): the context menu shows it as
 *   a submenu of the saved rules: "All rules that are on" (when there are
 *   several), the rules that are on, then under "Off" the rules that are
 *   off (running a rule by hand works for those too). Without rules it
 *   offers to open the rules window. Rule names are labels as written
 *   (`literalMenuLabel`: a rule named "@home" is not a Fluent id). The
 *   action itself only says whether the menu's target can run rules.
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
import type { ResolvedMenuEntry, Rule, RuleId } from "../../api/generated";
import type { UiStore } from "../../state/uiStore";
import { contextTarget, literalMenuLabel, type MenuSource } from "../menus";
import type { Feature } from "../types";
import { rulesKeys, rulesQuery, runRules } from "./api";
import { ruleRunTargetOf, type RunTarget } from "./runTargets";
import { rulesStore } from "./store";

export const RUN_MENU_ACTION = "rules.run-menu";
export const RUN_ACTION = "rules.run";
/** The `rules.run` argument for every rule that is on. */
export const ALL_RULES = "*";

/** Opens the rules window (also "No rules yet" of the rules menu). */
export const OPEN_RULES_ACTION = "rules.open";

/** What a menu's run acts on: a folder or list rows (never the rules' own mail). */
export type MenuRunTarget = Exclude<RunTarget, { kind: "scope" }>;

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
  const ruleItem = (r: Rule) => runItem(r.id, literalMenuLabel(r.name));
  out.push(...on.map(ruleItem));
  if (off.length > 0) {
    if (on.length > 0) out.push({ type: "separator" });
    out.push({ type: "header", label: "@rules.menu.off" }, ...off.map(ruleItem));
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

/** Refetches the run histories after runs or undos. */
export function refreshRuns(queryClient: QueryClient): Promise<void> {
  return queryClient.invalidateQueries({ queryKey: rulesKeys.allRuns });
}

/**
 * The rules submenu of "Run rule" for the open menu's target; nothing (the
 * item disabled) when the target cannot run rules or the rules cannot be
 * read (the toast says why).
 */
export const runMenuSource: MenuSource = {
  load: async (ctx) => {
    if (menuRunTarget(ctx.store) === null) return [];
    try {
      return runMenuItems(await savedRules(ctx));
    } catch (error) {
      rulesStore(ctx.client)
        .getState()
        .showToast({ kind: "error", id: errorMessageIdOf(error) });
      return [];
    }
  },
};

export const runMenuActions: Feature["actions"] = {
  // The item of the rules submenu (`runMenuSource`): running it opens nothing.
  [RUN_MENU_ACTION]: {
    run: () => undefined,
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
        const runTarget = await ruleRunTargetOf(ctx.client, target);
        if (runTarget === null) return;
        const runs = await runRules(ctx.client, ids, runTarget);
        toast.showToast({ kind: "ran", runs });
        await refreshRuns(ctx.queryClient);
      } catch (error) {
        toast.showToast({ kind: "error", id: errorMessageIdOf(error) });
      }
    },
    enabled: (ctx, args) => typeof args[0] === "string" && menuRunTarget(ctx.store) !== null,
  },
};
