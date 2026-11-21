/**
 * The multi-selection of a list box (R3, ADR 0229): which items are
 * selected, which one has the focus and the anchor a range extends from.
 * Pure functions over ids in list order, so the keyboard and the pointer
 * share one model. Used by the rule list and the contact list.
 *
 * - Click: only that item. Ctrl+click (⌘ on macOS): adds or removes it.
 *   Shift+click, Shift+Up / Down: the items from the anchor to it.
 * - Ctrl+A: every listed item (`pickAll`).
 */

/** The selection of a list of ids. */
export interface ListSelection {
  /** The selected ids (in the order they were picked; see `inListOrder`). */
  selected: readonly string[];
  /** Where Shift extends a range from; `null` before the first pick. */
  anchor: string | null;
  /** The id that has the focus . */
  focus: string;
}

/** How a pick changes the selection. */
export type SelectGesture = "single" | "toggle" | "range";

/** The gesture of a click or key press: Shift extends, Ctrl / ⌘ toggles. */
export function gestureOf(event: {
  shiftKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
}): SelectGesture {
  if (event.shiftKey) return "range";
  return event.ctrlKey || event.metaKey ? "toggle" : "single";
}

/** `ids` in the order of `order` (ids not in it are left out). */
export function inListOrder(order: readonly string[], ids: readonly string[]): string[] {
  const picked = new Set(ids);
  return order.filter((id) => picked.has(id));
}

/** The selection after picking `id` of `order` with `gesture`. */
export function pick(
  order: readonly string[],
  current: Pick<ListSelection, "selected" | "anchor">,
  id: string,
  gesture: SelectGesture,
): ListSelection {
  switch (gesture) {
    case "toggle": {
      const selected = current.selected.includes(id)
        ? current.selected.filter((s) => s !== id)
        : [...current.selected, id];
      return { selected, anchor: id, focus: id };
    }
    case "range": {
      const anchor =
        current.anchor !== null && order.includes(current.anchor) ? current.anchor : id;
      const a = order.indexOf(anchor);
      const b = order.indexOf(id);
      return {
        selected: order.slice(Math.min(a, b), Math.max(a, b) + 1),
        anchor,
        focus: id,
      };
    }
    case "single":
      return { selected: [id], anchor: id, focus: id };
  }
}

/** Every id of `order`, the focus kept (else the first). */
export function pickAll(order: readonly string[], focus: string | null): ListSelection | null {
  const first = order[0];
  if (first === undefined) return null;
  const kept = focus !== null && order.includes(focus) ? focus : first;
  return { selected: [...order], anchor: kept, focus: kept };
}

/** The id `step` places from `focus` in `order` (clamped); the first when nothing has the focus. */
export function stepFrom(
  order: readonly string[],
  focus: string | null,
  step: number,
): string | undefined {
  const index = focus === null ? -1 : order.indexOf(focus);
  if (index < 0) return order[0];
  return order[Math.max(0, Math.min(order.length - 1, index + step))];
}
