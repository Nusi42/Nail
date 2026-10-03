/**
 * How a plug-in's suggested placements become layout text (ADR 0050): the
 * mock core's answer to `PreviewPluginPlacements`, and the reference the
 * core's implementation (`nail-config/src/plugins.rs`) follows.
 *
 * - toolbar / menu placement: `item "plugin.<id>.<command>"` after (or
 *   before) the item with the named action, else at the end; a toolbar or
 *   menu that does not exist yet is created with just that item; an item
 *   already there is not added twice;
 * - key: `bind "<key>" "plugin.<id>.<command>"` in its context, unless the
 *   layout already binds that key in that context (the user's key wins);
 * - panel: `plugin-panel "<id>.<panel>"` after (or before) the first widget
 *   of the named kind, else at the end of the main window. The mock only
 *   notes panels as comments (it has no window text to insert into); the
 *   core writes them into the window tree.
 *
 * Only the sections that change are written (`layout "<name>" { … }`), so
 * the merge preview shows exactly the proposed items.
 */
import type {
  JsonValue,
  MenuItem,
  PluginInfo,
  PluginPlacement,
  ResolvedLayout,
  ToolbarItem,
} from "../../api/generated";
import { pluginCommandId } from "./ids";

function quote(text: string): string {
  return JSON.stringify(text);
}

function argsText(args: readonly JsonValue[]): string {
  return args.map((a) => ` ${typeof a === "string" ? quote(a) : JSON.stringify(a)}`).join("");
}

type Item = { type: "action"; action: string; args: JsonValue[] } | { type: "other"; text: string };

function toolbarItems(items: readonly ToolbarItem[]): Item[] {
  return items.map((i) =>
    i.type === "action"
      ? { type: "action", action: i.action, args: i.args }
      : { type: "other", text: i.type },
  );
}

function menuItemText(item: MenuItem, indent: string): string {
  switch (item.type) {
    case "action":
      return `${indent}item ${quote(item.action)}${argsText(item.args)}`;
    case "separator":
      return `${indent}separator`;
    case "header":
      return `${indent}header ${quote(item.label)}`;
    case "submenu":
      return [
        `${indent}submenu ${quote(item.label)} {`,
        ...item.items.map((i) => menuItemText(i, `${indent}    `)),
        `${indent}}`,
      ].join("\n");
  }
}

function menuItems(items: readonly MenuItem[]): Item[] {
  return items.map((i) =>
    i.type === "action"
      ? { type: "action", action: i.action, args: i.args }
      : { type: "other", text: menuItemText(i, "").trimStart() },
  );
}

/** Inserts `action` after / before the named action (else at the end). */
export function insertItem(items: Item[], action: string, placement: PluginPlacement): Item[] {
  if (items.some((i) => i.type === "action" && i.action === action)) return items;
  const item: Item = { type: "action", action, args: [] };
  const anchor = placement.after ?? placement.before;
  const at =
    anchor === undefined ? -1 : items.findIndex((i) => i.type === "action" && i.action === anchor);
  const next = [...items];
  if (at < 0) next.push(item);
  else next.splice(placement.after !== undefined ? at + 1 : at, 0, item);
  return next;
}

function itemsText(items: readonly Item[], indent: string): string[] {
  return items.map((i) =>
    i.type === "action"
      ? `${indent}item ${quote(i.action)}${argsText(i.args)}`
      : `${indent}${i.text}`,
  );
}

/**
 * The layout text proposing `plugin`'s placements for `layout`; `null`
 * when nothing would change.
 */
export function placementText(layout: ResolvedLayout, plugin: PluginInfo): string | null {
  const contributes = plugin.manifest.contributes;
  const toolbars = new Map<string, Item[]>();
  const menus = new Map<string, Item[]>();
  const panels: string[] = [];
  for (const placement of contributes?.placements ?? []) {
    if (placement.toolbar !== undefined && placement.command !== undefined) {
      const current =
        toolbars.get(placement.toolbar) ?? toolbarItems(layout.toolbars[placement.toolbar] ?? []);
      toolbars.set(
        placement.toolbar,
        insertItem(current, pluginCommandId(plugin.id, placement.command), placement),
      );
    } else if (placement.menu !== undefined && placement.command !== undefined) {
      const current = menus.get(placement.menu) ?? menuItems(layout.menus[placement.menu] ?? []);
      menus.set(
        placement.menu,
        insertItem(current, pluginCommandId(plugin.id, placement.command), placement),
      );
    } else if (placement.panel !== undefined) {
      const anchor = placement.after ?? placement.before;
      const where =
        anchor === undefined
          ? ""
          : ` // ${placement.after !== undefined ? "after" : "before"} ${anchor}`;
      panels.push(`plugin-panel ${quote(`${plugin.id}.${placement.panel}`)}${where}`);
    }
  }
  const keys = new Map<string, string[]>();
  for (const key of contributes?.keys ?? []) {
    const context = key.context ?? "global";
    const bound = layout.keys.some((k) => k.context === context && k.key === key.key);
    if (bound) continue;
    const list = keys.get(context) ?? [];
    list.push(`bind ${quote(key.key)} ${quote(pluginCommandId(plugin.id, key.command))}`);
    keys.set(context, list);
  }
  const changedToolbars = [...toolbars].filter(
    ([name, items]) =>
      JSON.stringify(items) !== JSON.stringify(toolbarItems(layout.toolbars[name] ?? [])),
  );
  const changedMenus = [...menus].filter(
    ([name, items]) =>
      JSON.stringify(items) !== JSON.stringify(menuItems(layout.menus[name] ?? [])),
  );
  if (
    changedToolbars.length === 0 &&
    changedMenus.length === 0 &&
    keys.size === 0 &&
    panels.length === 0
  ) {
    return null;
  }
  const lines = [`layout ${quote(layout.name)} {`];
  for (const [name, items] of changedToolbars) {
    lines.push(`    toolbar ${quote(name)} {`, ...itemsText(items, "        "), "    }");
  }
  for (const [name, items] of changedMenus) {
    lines.push(`    menu ${quote(name)} {`, ...itemsText(items, "        "), "    }");
  }
  for (const [context, binds] of keys) {
    const head = context === "global" ? "    keys {" : `    keys context=${quote(context)} {`;
    lines.push(head, ...binds.map((b) => `        ${b}`), "    }");
  }
  for (const panel of panels) lines.push(`    // ${panel}`);
  lines.push("}");
  return `${lines.join("\n")}\n`;
}
