/**
 * Toolbar overflow (M5b, ADR 0109) — the UI side of
 * `nail_api::views::overflow_order` plus the fitting step the renderer
 * needs; pure, so every toolbar (reader bar, main toolbar, calendar
 * toolbar) shares one behaviour:
 *
 * - `hide_disabled` toolbars drop actions that cannot run (in the "More"
 *   menu too) before anything is measured;
 * - when the items do not fit, they move into "More" in `overflowOrder`
 *   (unranked from the end first, then the highest rank; `rank=1` stays
 *   longest) until the rest plus the "More" button fits;
 * - separators and spacers that end up leading, trailing or doubled are
 *   dropped from the visible bar.
 */
import type { MenuItem, ToolbarItem, ToolbarSettings } from "../api/generated";

/** `overflow_order`: indices of action items in the order they move. */
export function overflowOrder(items: readonly ToolbarItem[]): number[] {
  const actions: { index: number; rank: number }[] = [];
  items.forEach((item, index) => {
    if (item.type === "action") {
      actions.push({ index, rank: item.rank ?? Number.POSITIVE_INFINITY });
    }
  });
  actions.sort((a, b) => b.rank - a.rank || b.index - a.index);
  return actions.map((a) => a.index);
}

/** What `fitToolbar` decided. */
export interface ToolbarFit {
  /** Indices shown in the bar, in bar order (separators normalized). */
  visible: number[];
  /** Indices of action items moved into "More", in bar order. */
  overflow: number[];
}

/**
 * Fits a toolbar into `available` pixels. `widths[i]` is item `i`'s width
 * (spacers count as 0); `moreWidth` is the "More" button's width, reserved
 * as soon as anything overflows (or always when the toolbar has its own
 * `more` item, whose width is then part of `widths`).
 */
export function fitToolbar(
  items: readonly ToolbarItem[],
  widths: readonly number[],
  available: number,
  moreWidth: number,
): ToolbarFit {
  const width = (i: number) => (items[i]?.type === "spacer" ? 0 : (widths[i] ?? 0));
  const hasMore = items.some((item) => item.type === "more");
  const moved = new Set<number>();
  const used = () =>
    items.reduce((sum, _item, i) => (moved.has(i) ? sum : sum + width(i)), 0) +
    (moved.size > 0 && !hasMore ? moreWidth : 0);
  for (const index of overflowOrder(items)) {
    if (used() <= available) break;
    moved.add(index);
  }
  const kept = items.map((_item, i) => i).filter((i) => !moved.has(i));
  return {
    visible: normalizeSeparators(items, kept),
    overflow: [...moved].sort((a, b) => a - b),
  };
}

/** Drops leading, trailing and doubled separators / spacers from `kept`. */
export function normalizeSeparators(
  items: readonly ToolbarItem[],
  kept: readonly number[],
): number[] {
  const out: number[] = [];
  for (const index of kept) {
    const type = items[index]?.type;
    const isGap = type === "separator" || type === "spacer";
    const last = out[out.length - 1];
    const lastType = last === undefined ? undefined : items[last]?.type;
    if (isGap && (last === undefined || lastType === "separator" || lastType === "spacer")) {
      // A spacer after a separator still pushes the rest to the end.
      if (type === "spacer" && lastType === "separator") out[out.length - 1] = index;
      continue;
    }
    out.push(index);
  }
  while (out.length > 0) {
    const tail = items[out[out.length - 1] ?? -1]?.type;
    if (tail !== "separator" && tail !== "spacer") break;
    out.pop();
  }
  return out;
}

function enabledMenu(
  items: readonly MenuItem[],
  isEnabled: (item: Extract<MenuItem, { type: "action" }>) => boolean,
): MenuItem[] {
  const out: MenuItem[] = [];
  for (const item of items) {
    if (item.type === "action") {
      if (isEnabled(item)) out.push(item);
    } else if (item.type === "submenu") {
      const children = enabledMenu(item.items, isEnabled);
      if (children.length > 0) out.push({ ...item, items: children });
    } else {
      out.push(item);
    }
  }
  return out;
}

/**
 * The items a toolbar shows: with `hide_disabled`, actions that cannot run
 * are left out (also inside its "More" menu); otherwise all items.
 */
export function shownToolbarItems(
  items: readonly ToolbarItem[],
  settings: ToolbarSettings,
  isEnabled: (action: string, args: Extract<ToolbarItem, { type: "action" }>["args"]) => boolean,
): ToolbarItem[] {
  if (!settings.hide_disabled) return [...items];
  const out: ToolbarItem[] = [];
  for (const item of items) {
    if (item.type === "action") {
      if (isEnabled(item.action, item.args)) out.push(item);
    } else if (item.type === "more") {
      out.push({
        ...item,
        items: enabledMenu(item.items, (a) => isEnabled(a.action, a.args)),
      });
    } else {
      out.push(item);
    }
  }
  return out;
}
