/**
 * Dynamic menu sources (R2, ADR 0222): a menu item whose entries are the
 * user's data (the saved rules of "Run rule") is a layout item like any
 * other — `item "<action>"` in `layout.kdl` — whose action has a source
 * here. The context menu shows such an item as a submenu that loads its
 * entries when it opens (hover, ArrowRight, Enter), with the menu's
 * target as context. A feature registers its sources as `menuSources`
 * (`../types.ts`, keyed by the action id); the central registry merges
 * them like actions.
 *
 * Labels: entries are `ResolvedMenuEntry`s, whose labels are read as
 * Fluent ids when they start with `@`. Labels that are user data (a rule
 * named "@home") go through `literalMenuLabel`, so they always show as
 * written.
 *
 * Contract stub: the types and `menuSourceFor`; showing a source as a
 * hover submenu and `literalMenuLabel` are the R1 leftovers package's
 * (WR6), which then moves "Run rule" onto a source.
 */
import type { ActionContext } from "../../actions/registry";
import type { ResolvedMenuEntry } from "../../api/generated";

/** The entries of a dynamic submenu. */
export interface MenuSource {
  /**
   * The entries for the open menu's target (read it from `ctx`); an empty
   * list shows the item disabled.
   */
  load: (ctx: ActionContext) => Promise<ResolvedMenuEntry[]>;
}

/** Menu sources by the action id of the layout item that opens them. */
export type MenuSourceRegistry = Readonly<Record<string, MenuSource>>;

/** The source of a layout item's action, or `null` for a plain item. */
export function menuSourceFor(registry: MenuSourceRegistry, action: string): MenuSource | null {
  return Object.hasOwn(registry, action) ? (registry[action] ?? null) : null;
}

/**
 * A label that shows exactly `text`, even when it starts with `@` (stub:
 * returns `text` until the menus read an escape).
 */
export function literalMenuLabel(text: string): string {
  return text;
}
