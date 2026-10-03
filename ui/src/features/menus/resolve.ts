/**
 * Context menus resolved from `layout.kdl` — the UI side of
 * `nail_api::menus::resolve_menu` (ADR 0043), same rules, tested against the
 * same cases (`resolve.test.ts` mirrors the Rust tests):
 *
 * - label and icon: the item's override, else the layout's action metadata,
 *   else `null` (the action's built-in label);
 * - shortcut: the last key binding of the same action and arguments in the
 *   part's key context, else the last `global` one;
 * - separators are normalized (no leading, trailing or doubled ones) and
 *   empty submenus are dropped; a menu without any action is `null`;
 * - headings (M5b) are kept only when an action or submenu follows them
 *   before the next heading or separator.
 */
import type {
  JsonValue,
  MenuItem,
  ResolvedLayout,
  ResolvedMenu,
  ResolvedMenuEntry,
} from "../../api/generated";

/** Prefix of context-menu names in the layout. */
export const CONTEXT_MENU_PREFIX = "context:";

/** Parts with context menus and the key context of their shortcuts. */
export const CONTEXT_MENU_PARTS: Readonly<Record<string, string>> = {
  "message-list.row": "list",
  "thread-view.message": "reader",
  "message-view": "reader",
  "message-view.attachment": "reader",
  "folder-tree.folder": "global",
  "folder-tree.account": "global",
  composer: "composer",
  // M5b: the "…" menu of a calendar in the calendar list.
  "calendar-list.calendar": "calendar",
};

/** `context:<part>`. */
export function contextMenuName(part: string): string {
  return `${CONTEXT_MENU_PREFIX}${part}`;
}

/** The key context of a part's context menu (`global` when unknown). */
export function keyContextOf(part: string): string {
  return CONTEXT_MENU_PARTS[part] ?? "global";
}

function sameArgs(a: readonly JsonValue[], b: readonly JsonValue[]): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** The key bound to `action` + `args` in `keyContext`, else in `global`. */
export function shortcut(
  layout: ResolvedLayout,
  action: string,
  args: readonly JsonValue[],
  keyContext: string,
): string | null {
  const find = (context: string) =>
    [...layout.keys]
      .reverse()
      .find((k) => k.context === context && k.action === action && sameArgs(k.args, args))?.key ??
    null;
  return find(keyContext) ?? find("global");
}

/** Removes a trailing heading nothing follows (`drop_orphan_header`). */
function dropOrphanHeader(out: ResolvedMenuEntry[]): void {
  if (out[out.length - 1]?.type === "header") out.pop();
}

function resolveItems(
  layout: ResolvedLayout,
  items: readonly MenuItem[],
  keyContext: string,
): ResolvedMenuEntry[] {
  const out: ResolvedMenuEntry[] = [];
  for (const item of items) {
    if (item.type === "action") {
      const def = layout.actions[item.action];
      out.push({
        type: "action",
        action: item.action,
        args: item.args,
        label: item.label ?? def?.label ?? null,
        icon: item.icon ?? def?.icon ?? null,
        shortcut: shortcut(layout, item.action, item.args, keyContext),
      });
    } else if (item.type === "separator") {
      dropOrphanHeader(out);
      const last = out[out.length - 1];
      if (last !== undefined && last.type !== "separator") out.push({ type: "separator" });
    } else if (item.type === "header") {
      dropOrphanHeader(out);
      out.push({ type: "header", label: item.label });
    } else {
      const children = resolveItems(layout, item.items, keyContext);
      if (children.length > 0) out.push({ type: "submenu", label: item.label, items: children });
    }
  }
  dropOrphanHeader(out);
  if (out[out.length - 1]?.type === "separator") out.pop();
  // Headings and separators alone have nothing to run.
  if (!out.some((e) => e.type === "action" || e.type === "submenu")) return [];
  return out;
}

/** Resolves the layout menu `name`; `null` when missing or without actions. */
export function resolveMenu(
  layout: ResolvedLayout,
  name: string,
  keyContext: string,
): ResolvedMenu | null {
  const items = layout.menus[name];
  if (!items) return null;
  const resolved = resolveItems(layout, items, keyContext);
  return resolved.length > 0 ? { name, items: resolved } : null;
}
