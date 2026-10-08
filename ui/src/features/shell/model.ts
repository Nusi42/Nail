/**
 * Pure rules of the shell (M5b WP161, ADR 0109): which rail entries render,
 * the key bindings that show a view, the keyboard context of a view, badge
 * numbers. The view rules shared with Rust (`step_view`, start view, …) live
 * in `../../layout/views.ts`; these are UI-only.
 */
import type {
  BadgeSource,
  Folder,
  JsonValue,
  RailItem,
  ResolvedLayout,
  ViewDef,
} from "../../api/generated";
import { VIEW_SHOW_ACTION, hostsViews, viewDef } from "../../layout/views";

/** Badges show at most this number, then `99+`. */
export const BADGE_CAP = 99;

/** A rail entry ready to render. */
export type RailEntry =
  | { type: "view"; id: string; def: ViewDef }
  | Extract<RailItem, { type: "action" }>
  | { type: "spacer" }
  | { type: "separator" };

/**
 * The rail's entries: views the layout defines (unknown ones are dropped —
 * `nail-config` refuses them, this keeps a stale layout harmless), actions,
 * spacers, and separators that sit between two buttons (never first, last,
 * next to a spacer or doubled).
 */
export function railEntries(layout: ResolvedLayout): RailEntry[] {
  const out: RailEntry[] = [];
  for (const item of layout.rail) {
    if (item.type === "view") {
      const def = viewDef(layout, item.view);
      if (def !== null) out.push({ type: "view", id: item.view, def });
    } else {
      out.push(item);
    }
  }
  const isButton = (e: RailEntry | undefined) =>
    e !== undefined && (e.type === "view" || e.type === "action");
  const kept: RailEntry[] = [];
  out.forEach((entry, i) => {
    if (entry.type !== "separator") {
      kept.push(entry);
      return;
    }
    // Between a kept button and the next entry that is not a separator.
    const next = out.slice(i + 1).find((e) => e.type !== "separator");
    if (isButton(kept[kept.length - 1]) && isButton(next)) kept.push(entry);
  });
  return kept;
}

/** The views a rail entry list shows, in order (for tests and focus order). */
export function railViews(entries: readonly RailEntry[]): string[] {
  return entries.flatMap((e) => (e.type === "view" ? [e.id] : []));
}

/**
 * The key sequences that show `view`: bindings of `view.show "<view>"` and
 * of an alias of it (`view.calendar` = `view.show "calendar"`). Global
 * bindings first.
 */
export function viewShortcuts(layout: ResolvedLayout, view: string): string[] {
  const showsView = (action: string, args: readonly JsonValue[]): boolean => {
    if (action === VIEW_SHOW_ACTION) return args[0] === view;
    const alias = Object.hasOwn(layout.actions, action) ? layout.actions[action] : undefined;
    return args.length === 0 && alias?.alias_of === VIEW_SHOW_ACTION && alias.args[0] === view;
  };
  const matching = layout.keys.filter((b) => showsView(b.action, b.args));
  const ordered = [
    ...matching.filter((b) => b.context === "global"),
    ...matching.filter((b) => b.context !== "global"),
  ];
  return [...new Set(ordered.map((b) => b.key))];
}

/** The key sequences bound to an action without arguments (rail action items). */
export function actionShortcuts(
  layout: ResolvedLayout,
  action: string,
  args: readonly JsonValue[],
): string[] {
  const same = (a: readonly JsonValue[]) => JSON.stringify(a) === JSON.stringify(args);
  return [
    ...new Set(layout.keys.filter((b) => b.action === action && same(b.args)).map((b) => b.key)),
  ];
}

/**
 * Keyboard contexts of the PIM views: inside them the context's bindings
 * apply (`d` / `w` / `m` in the calendar) and the mail list's do not.
 */
const VIEW_KEY_CONTEXTS = new Set(["calendar", "contacts", "tasks"]);

/**
 * The keyboard context of a view's region (`data-key-context`): the PIM
 * context named like the view or its window, else `global` — so a hidden
 * mail list never answers `j` / `k` while another view shows, and widgets
 * inside keep their own contexts.
 */
export function viewKeyContext(id: string, def: ViewDef): string {
  if (VIEW_KEY_CONTEXTS.has(id)) return id;
  if (VIEW_KEY_CONTEXTS.has(def.window)) return def.window;
  return "global";
}

/**
 * Whether the host can show `view`: its window exists and does not host
 * views itself (`nail-config` refuses that; this guards against recursion).
 */
export function canHost(layout: ResolvedLayout, view: string): boolean {
  const def = viewDef(layout, view);
  return (
    def !== null && Object.hasOwn(layout.windows, def.window) && !hostsViews(layout, def.window)
  );
}

/** The remembered view from the UI state, if the layout still defines it. */
export function rememberedView(layout: ResolvedLayout, value: JsonValue | null): string | null {
  return typeof value === "string" && viewDef(layout, value) !== null ? value : null;
}

/** Unread messages in the inboxes: the "All accounts" inbox(es). */
export function inboxUnread(folders: readonly Folder[] | undefined): number {
  return (folders ?? []).filter((f) => f.role === "inbox").reduce((sum, f) => sum + f.unread, 0);
}

/** The badge sources a layout's rail uses (views' and action items' badges). */
export function badgeSources(entries: readonly RailEntry[]): Set<BadgeSource> {
  const out = new Set<BadgeSource>();
  for (const entry of entries) {
    const badge =
      entry.type === "view" ? entry.def.badge : entry.type === "action" ? entry.badge : null;
    if (badge !== null) out.add(badge);
  }
  return out;
}

/** The Fluent message (id + args) of a badge's visible text. */
export function badgeText(count: number): { id: string; args: { count: number } } {
  return count > BADGE_CAP
    ? { id: "shell-rail-badge-many", args: { count: BADGE_CAP } }
    : { id: "shell-rail-badge-count", args: { count } };
}

/** The Fluent id of a rail button's accessible name with a badge. */
export function badgeLabelId(source: BadgeSource): string {
  return `shell-rail-item-${source}`;
}

/** Every badge source (for the catalog coverage). */
export const BADGE_SOURCES: readonly BadgeSource[] = ["unread", "confirmations"];
