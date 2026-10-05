/**
 * The mounted expandable message lists (P2 WP202, Design B), for the list
 * actions `list.expand` / `list.collapse` (→ / ←, registered by
 * `features/conversation-list`): an action works on the list that holds
 * the focus, else on the one mounted last.
 */

/** What an expandable list offers its actions. */
export interface ListControl {
  /** The list's element (to find the focused one). */
  element: () => HTMLElement | null;
  /** → on the selected row. */
  expand: () => void;
  /** ← on the selected row. */
  collapse: () => void;
}

const controls: ListControl[] = [];

/** Registers a mounted list; the returned function unregisters it. */
export function registerListControl(control: ListControl): () => void {
  controls.push(control);
  return () => {
    const index = controls.indexOf(control);
    if (index >= 0) controls.splice(index, 1);
  };
}

/** The list the list actions act on, or `null` without an expandable list. */
export function currentListControl(): ListControl | null {
  const active = typeof document === "undefined" ? null : document.activeElement;
  if (active) {
    const focused = controls.find((c) => c.element()?.contains(active) === true);
    if (focused) return focused;
  }
  return controls[controls.length - 1] ?? null;
}
