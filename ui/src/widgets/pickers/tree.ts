/**
 * The shared pickers' list model (R2, ADR 0222): which rows of a tree are
 * shown (closed rows hide their descendants; while filtering every match
 * shows), the row that becomes active when the list opens, and how the
 * keyboard moves the active row (Up / Down, Home / End, Left to the parent
 * or closing a row, Right into a row's children). Flat lists are trees of
 * depth 0. Pure functions: the `Picker` keeps the state.
 */
import { optionKey } from "./sources";
import type { PickerOption } from "./types";

/** Keys of the rows that have children. */
export function parentKeys(options: readonly PickerOption[]): Set<string> {
  const parents = new Set<string>();
  for (const option of options) if (option.parent !== null) parents.add(option.parent);
  return parents;
}

/**
 * The rows shown: every row while `filtering` (matches keep their parents
 * open), else the rows whose ancestors are all open.
 */
export function visibleRows(
  options: readonly PickerOption[],
  collapsed: ReadonlySet<string>,
  filtering: boolean,
): PickerOption[] {
  if (filtering || collapsed.size === 0) return [...options];
  const hidden = new Set<string>();
  return options.filter((option) => {
    const parent = option.parent;
    if (parent !== null && (collapsed.has(parent) || hidden.has(parent))) {
      hidden.add(optionKey(option));
      return false;
    }
    return true;
  });
}

/** Whether a row can become the field's value (account rows of a folder tree cannot). */
export function pickable(option: PickerOption): boolean {
  return option.variant !== "group";
}

/**
 * The row active when the list opens: while browsing the field's `value`,
 * while typing the first row whose name starts with the `typed` text (the
 * rows may still be the ones of an earlier, shorter text), else the first
 * pickable row.
 */
export function initialActive(
  rows: readonly PickerOption[],
  value: string | null,
  typed: string | null = null,
): string | null {
  const candidates = rows.filter((r) => pickable(r) && r.variant !== "create");
  const needle = typed?.trim().toLowerCase() ?? "";
  const current =
    value === null
      ? needle === ""
        ? undefined
        : candidates.find((r) => r.label.toLowerCase().startsWith(needle))
      : candidates.find((r) => r.value === value);
  const first = current ?? rows.find(pickable) ?? rows[0];
  return first ? optionKey(first) : null;
}

/** How a key moves the active row. */
export type TreeMove = "next" | "previous" | "first" | "last" | "parent" | "child";

/** The row `move` makes active (`null` keeps none); the active row stays when it cannot move. */
export function moveActive(
  rows: readonly PickerOption[],
  active: string | null,
  move: TreeMove,
): string | null {
  if (rows.length === 0) return null;
  const keys = rows.map(optionKey);
  const at = active === null ? -1 : keys.indexOf(active);
  const key = (index: number) => keys[Math.max(0, Math.min(keys.length - 1, index))] ?? null;
  switch (move) {
    case "next":
      return key(at < 0 ? 0 : at + 1);
    case "previous":
      return key(at < 0 ? keys.length - 1 : at - 1);
    case "first":
      return key(0);
    case "last":
      return key(keys.length - 1);
    case "parent": {
      const parent = rows[at]?.parent ?? null;
      return parent !== null && keys.includes(parent) ? parent : active;
    }
    case "child": {
      const next = active === null ? undefined : rows[at + 1];
      if (next === undefined) return active;
      return next.parent === active ? optionKey(next) : active;
    }
  }
}

/** `collapsed` with `key` opened or closed. */
export function toggled(collapsed: ReadonlySet<string>, key: string): Set<string> {
  const next = new Set(collapsed);
  if (next.has(key)) next.delete(key);
  else next.add(key);
  return next;
}
