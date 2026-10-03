/**
 * Pure helpers of the toolbar widget (M5b WP162, ADRs 0109 / 0111):
 * how a button shows its action (`display`), its tooltip with the key
 * binding, the entries of the "More" menu (its own entries plus what did not
 * fit), and which separators start or end a wrapped line. The fitting rule
 * itself is shared by every toolbar: `../layout/overflow.ts`.
 */
import type {
  ItemDisplay,
  JsonValue,
  MenuItem,
  ResolvedLayout,
  ResolvedMenuEntry,
  ToolbarItem,
  ToolbarSettings,
} from "../api/generated";
import { DEFAULT_TOOLBAR_SETTINGS } from "../api/normalize";
import { resolveMenu } from "../features/menus";

/** What a toolbar without its own settings does (`ToolbarSettings::default()`). */
export { DEFAULT_TOOLBAR_SETTINGS };

/** The settings of toolbar `name` (defaults when the layout has none). */
export function toolbarSettingsOf(layout: ResolvedLayout, name: string): ToolbarSettings {
  const all = layout.toolbar_settings as ResolvedLayout["toolbar_settings"] | undefined;
  return all && Object.hasOwn(all, name)
    ? (all[name] ?? DEFAULT_TOOLBAR_SETTINGS)
    : DEFAULT_TOOLBAR_SETTINGS;
}

/** How an action item shows: its `display`, else icon and label. */
export function displayOf(item: Extract<ToolbarItem, { type: "action" }>): ItemDisplay {
  return item.display ?? "icon-label";
}

function sameArgs(a: readonly JsonValue[], b: readonly JsonValue[]): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/**
 * The key bound to `action` + `args` in `keyContext`, else in `global`
 * (the last binding wins, like the key matcher), `null` when none.
 */
export function bindingOf(
  layout: ResolvedLayout,
  action: string,
  args: readonly JsonValue[],
  keyContext: string,
): string | null {
  const find = (context: string) => {
    for (let i = layout.keys.length - 1; i >= 0; i--) {
      const k = layout.keys[i];
      if (k && k.context === context && k.action === action && sameArgs(k.args, args)) {
        return k.key;
      }
    }
    return null;
  };
  return (keyContext !== "global" ? find(keyContext) : null) ?? find("global");
}

/** The Fluent id + arguments of a button's tooltip ("Archive (E)"). */
export function tooltipMessage(
  label: string,
  shortcutText: string | null,
): { id: string; args: Record<string, string> } | null {
  return shortcutText === null
    ? null
    : { id: "toolbar-button-tooltip", args: { label, shortcut: shortcutText } };
}

/** The menu name the "More" menu resolves under (never a layout menu). */
const MORE_MENU = "toolbar:more";

/**
 * The "More" menu's entries: the `more` item's own entries, then — after a
 * separator — the actions that did not fit into the bar, in bar order
 * (ADR 0109: "overflowing items are appended"). Resolved like a context
 * menu (labels, icons, shortcuts, headings kept only above actions,
 * separators normalized); empty when nothing would run.
 */
export function moreMenuEntries(
  layout: ResolvedLayout,
  items: readonly ToolbarItem[],
  overflow: readonly number[],
  keyContext: string,
): ResolvedMenuEntry[] {
  const own = items.find((item) => item.type === "more");
  const moved: MenuItem[] = [];
  for (const index of overflow) {
    const item = items[index];
    if (item?.type !== "action") continue;
    moved.push({
      type: "action",
      action: item.action,
      args: item.args,
      label: item.label,
      icon: item.icon,
    });
  }
  const entries: MenuItem[] = [
    ...(own?.type === "more" ? own.items : []),
    ...(moved.length > 0 ? [{ type: "separator" } as const, ...moved] : []),
  ];
  const resolved = resolveMenu(
    { ...layout, menus: { [MORE_MENU]: entries } },
    MORE_MENU,
    keyContext,
  );
  return resolved?.items ?? [];
}

/**
 * For a wrapping toolbar: which separators sit at the start or the end of a
 * line, given each rendered item's kind and top offset (in bar order). A
 * separator starting a line is removed (removing it never changes the lines
 * above it); one ending a line is hidden but keeps its width (so nothing
 * moves up and the decision stays stable).
 */
export function wrapEdges(
  rendered: readonly { kind: ToolbarItem["type"]; top: number }[],
): ("start" | "end" | null)[] {
  return rendered.map((item, i) => {
    if (item.kind !== "separator") return null;
    const prev = rendered[i - 1];
    const next = rendered[i + 1];
    if (prev === undefined || prev.top !== item.top) return "start";
    if (next === undefined || next.top !== item.top) return "end";
    return null;
  });
}
