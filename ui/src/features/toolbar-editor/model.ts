/**
 * The toolbar customizer's model (M5d WP182, ADR 0131): a toolbar's items
 * split into the bar and its "More" menu, which item may go where, the
 * patches a drag or a key press sends, and how those patches change a
 * toolbar (the mock core applies them with {@link applyToolbarEdit}, the
 * real core in nail-config's `layout_edit.rs`).
 *
 * Slots (`ToolbarSlot`): on the bar the index counts the bar's items without
 * the `more {}` item; in "More" it counts the menu's own entries. A
 * destination index is read after the item left its place (`move` patches)
 * and is clamped to the end.
 */
import type {
  LayoutPatch,
  MenuItem,
  ToolbarArea,
  ToolbarItem,
  ToolbarSlot,
} from "../../api/generated";

type MoreItem = Extract<ToolbarItem, { type: "more" }>;
type ActionItem = Extract<ToolbarItem, { type: "action" }>;

/** Anything shown in the customizer's bar or "More" list. */
export type CustomizerItem = ToolbarItem | MenuItem;

/** The bar's items (without the `more {}` item), in order. */
export function barItems(items: readonly ToolbarItem[]): ToolbarItem[] {
  return items.filter((item) => item.type !== "more");
}

/** The `more {}` item, if the toolbar has one. */
export function moreItemOf(items: readonly ToolbarItem[]): MoreItem | null {
  return items.find((item): item is MoreItem => item.type === "more") ?? null;
}

/** The "More" menu's own entries, in order. */
export function moreItems(items: readonly ToolbarItem[]): MenuItem[] {
  return moreItemOf(items)?.items ?? [];
}

/** The items of one area. */
export function areaItems(items: readonly ToolbarItem[], area: ToolbarArea): CustomizerItem[] {
  return area === "bar" ? barItems(items) : moreItems(items);
}

/**
 * Whether `item` may sit in `area`: the bar takes actions, separators and
 * spacers; "More" takes actions, separators, headings and submenus (a spacer
 * means nothing in a menu).
 */
export function canPlace(item: CustomizerItem, area: ToolbarArea): boolean {
  switch (item.type) {
    case "action":
    case "separator":
      return true;
    case "spacer":
      return area === "bar";
    case "header":
    case "submenu":
      return area === "more";
    case "more":
      return false;
  }
}

/** An item as a menu entry ("More"): an action keeps its label and icon. */
export function asMenuItem(item: CustomizerItem): MenuItem | null {
  switch (item.type) {
    case "action":
      return {
        type: "action",
        action: item.action,
        args: structuredClone(item.args),
        label: item.label,
        icon: item.icon,
      };
    case "separator":
      return { type: "separator" };
    case "header":
    case "submenu":
      return structuredClone(item);
    case "spacer":
    case "more":
      return null;
  }
}

/** An item as a bar item: a menu action keeps its label and icon. */
export function asToolbarItem(item: CustomizerItem): ToolbarItem | null {
  switch (item.type) {
    case "action": {
      const out: ActionItem = {
        type: "action",
        action: item.action,
        args: structuredClone(item.args),
        label: item.label,
        icon: item.icon,
      };
      // A bar action keeps its rank and display (a menu action has neither).
      if ("rank" in item) out.rank = item.rank;
      if ("display" in item) out.display = item.display;
      return out;
    }
    case "separator":
      return { type: "separator" };
    case "spacer":
      return { type: "spacer" };
    case "header":
    case "submenu":
    case "more":
      return null;
  }
}

/**
 * The destination slot of a move that drops the item at `from` before the
 * item now at `before` (or at the end of `before.area` when `before.index`
 * is the area's length): indexes of the destination are counted after the
 * item left its place.
 */
export function moveDestination(from: ToolbarSlot, before: ToolbarSlot): ToolbarSlot {
  if (from.area === before.area && from.index < before.index) {
    return { area: before.area, index: before.index - 1 };
  }
  return { ...before };
}

/** Whether a move changes nothing (the item dropped onto its own place). */
export function isNoOpMove(from: ToolbarSlot, to: ToolbarSlot): boolean {
  return from.area === to.area && from.index === to.index;
}

/** `move-toolbar-item`. */
export function movePatch(
  toolbar: string,
  from: ToolbarSlot,
  to: ToolbarSlot,
): Extract<LayoutPatch, { op: "move-toolbar-item" }> {
  return { op: "move-toolbar-item", toolbar, from: { ...from }, to: { ...to } };
}

/** `add-toolbar-item`. */
export function addPatch(
  toolbar: string,
  item: ToolbarItem,
  to: ToolbarSlot,
): Extract<LayoutPatch, { op: "add-toolbar-item" }> {
  return { op: "add-toolbar-item", toolbar, item: structuredClone(item), to: { ...to } };
}

/** `remove-toolbar-item`. */
export function removePatch(
  toolbar: string,
  from: ToolbarSlot,
): Extract<LayoutPatch, { op: "remove-toolbar-item" }> {
  return { op: "remove-toolbar-item", toolbar, from: { ...from } };
}

/** `reset-toolbar` ("Reset to default"). */
export function resetPatch(toolbar: string): LayoutPatch {
  return { op: "reset-toolbar", toolbar };
}

/** The patches this model applies. */
export type ToolbarEdit = Extract<
  LayoutPatch,
  { op: "move-toolbar-item" | "add-toolbar-item" | "remove-toolbar-item" }
>;

/** Why an edit was refused (the core answers `edit-issue-toolbar-slot-invalid`). */
export class ToolbarEditError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ToolbarEditError";
  }
}

function clamp(index: number, length: number): number {
  return Math.max(0, Math.min(Math.trunc(index), length));
}

/** Inserts into the bar before its `index`-th item (after the last one at the end). */
function insertIntoBar(items: ToolbarItem[], index: number, item: ToolbarItem): void {
  const positions = items.flatMap((it, i) => (it.type === "more" ? [] : [i]));
  const at = clamp(index, positions.length);
  if (at < positions.length) {
    items.splice(positions[at] ?? items.length, 0, item);
    return;
  }
  const last = positions.at(-1);
  // No bar items yet: before the `more {}` item, if any.
  const more = items.findIndex((it) => it.type === "more");
  items.splice(last === undefined ? (more < 0 ? items.length : more) : last + 1, 0, item);
}

/** Takes the item at `from` out of `items` (returns it). */
function take(items: ToolbarItem[], from: ToolbarSlot): CustomizerItem {
  if (from.area === "bar") {
    const positions = items.flatMap((it, i) => (it.type === "more" ? [] : [i]));
    const position = positions[from.index];
    if (position === undefined || !Number.isInteger(from.index)) {
      throw new ToolbarEditError(`no item at bar ${String(from.index)}`);
    }
    const [removed] = items.splice(position, 1);
    if (removed === undefined) throw new ToolbarEditError("no item");
    return removed;
  }
  const more = items.find((it): it is MoreItem => it.type === "more");
  const removed = Number.isInteger(from.index) ? more?.items[from.index] : undefined;
  if (more === undefined || removed === undefined) {
    throw new ToolbarEditError(`no item at more ${String(from.index)}`);
  }
  more.items.splice(from.index, 1);
  return removed;
}

/** Puts `item` at `to` (a `more {}` item is added at the end when missing). */
function put(items: ToolbarItem[], item: CustomizerItem, to: ToolbarSlot): void {
  if (!canPlace(item, to.area)) {
    throw new ToolbarEditError(`a ${item.type} cannot go into ${to.area}`);
  }
  if (to.area === "bar") {
    const converted = asToolbarItem(item);
    if (converted === null) throw new ToolbarEditError(`a ${item.type} cannot go on the bar`);
    insertIntoBar(items, to.index, converted);
    return;
  }
  const converted = asMenuItem(item);
  if (converted === null) throw new ToolbarEditError(`a ${item.type} cannot go into More`);
  let more = items.find((it): it is MoreItem => it.type === "more");
  if (more === undefined) {
    more = { type: "more", label: null, icon: null, items: [] };
    items.push(more);
  }
  more.items.splice(clamp(to.index, more.items.length), 0, converted);
}

/**
 * A toolbar's items after `edit` (the input is not changed). Throws
 * {@link ToolbarEditError} for a slot without an item, an item that cannot
 * go where it was sent, or a `more` item added from outside.
 */
export function applyToolbarEdit(items: readonly ToolbarItem[], edit: ToolbarEdit): ToolbarItem[] {
  const next = structuredClone(items) as ToolbarItem[];
  switch (edit.op) {
    case "remove-toolbar-item":
      take(next, edit.from);
      return next;
    case "add-toolbar-item":
      if (edit.item.type === "more") throw new ToolbarEditError("a more item cannot be added");
      put(next, edit.item, edit.to);
      return next;
    case "move-toolbar-item": {
      const item = take(next, edit.from);
      put(next, item, edit.to);
      return next;
    }
  }
}

/** The action ids a toolbar already shows (bar and "More", submenus included). */
export function usedActions(items: readonly ToolbarItem[]): Set<string> {
  const out = new Set<string>();
  const visit = (entries: readonly CustomizerItem[]) => {
    for (const entry of entries) {
      if (entry.type === "action") out.add(entry.action);
      else if (entry.type === "submenu" || entry.type === "more") visit(entry.items);
    }
  };
  visit(items);
  return out;
}
