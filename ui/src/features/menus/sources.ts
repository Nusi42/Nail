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
 * written: it marks them with {@link LITERAL_LABEL_MARK}, which the menu
 * drops instead of resolving the label (`menuLabelText`).
 *
 * The context menu loads every source of a list when the list appears
 * (`MenuSourceLoader`), so an empty source shows its item disabled before
 * it is hovered.
 */
import { createContext } from "react";
import type { ActionContext } from "../../actions/registry";
import type { ResolvedMenuEntry } from "../../api/generated";

/** The entries of a dynamic submenu. */
export interface MenuSource {
  /**
   * The entries for the open menu's target (read it from `ctx`); an empty
   * list shows the item disabled, unless `runsWhenEmpty`.
   */
  load: (ctx: ActionContext) => Promise<ResolvedMenuEntry[]>;
  /** Without entries the item is a plain item that runs its own action (default: disabled). */
  runsWhenEmpty?: boolean;
}

/** Menu sources by the action id of the layout item that opens them. */
export type MenuSourceRegistry = Readonly<Record<string, MenuSource>>;

/**
 * The app's menu sources (`AppRegistry.menuSources`, provided by the app
 * shell): the context menu cannot import the registry, which imports it.
 */
export const MenuSourcesContext = createContext<MenuSourceRegistry>({});

/** The source of a layout item's action, or `null` for a plain item. */
export function menuSourceFor(registry: MenuSourceRegistry, action: string): MenuSource | null {
  return Object.hasOwn(registry, action) ? (registry[action] ?? null) : null;
}

/**
 * The mark of a literal menu label: a label starting with it shows the rest
 * as written. Labels that already start with it are marked too, so every
 * text round-trips.
 */
export const LITERAL_LABEL_MARK = "\\";

/** A label that shows exactly `text`, even when it starts with `@`. */
export function literalMenuLabel(text: string): string {
  return text.startsWith("@") || text.startsWith(LITERAL_LABEL_MARK)
    ? `${LITERAL_LABEL_MARK}${text}`
    : text;
}

/**
 * The text a menu shows for `label`: a literal label as written (without
 * its mark), else the label resolved (`@a.b` is a Fluent id).
 */
export function menuLabelText(label: string, resolve: (label: string) => string): string {
  return label.startsWith(LITERAL_LABEL_MARK)
    ? label.slice(LITERAL_LABEL_MARK.length)
    : resolve(label);
}

/**
 * What the context menu needs to show sources: whether an action has one,
 * and its entries for the open menu (the host binds the menu's context and
 * drops the entries that do not apply).
 */
export interface MenuSourceLoader {
  has: (action: string) => boolean;
  load: (action: string) => Promise<ResolvedMenuEntry[]>;
  /** Whether the item runs its own action when its source has no entries. */
  runsWhenEmpty?: (action: string) => boolean;
}
