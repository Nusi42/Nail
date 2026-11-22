/**
 * The mounted message lists, for the list actions that act on the list's
 * own rows: `list.expand` / `list.collapse` (→ / ← in an expandable list,
 * P2 WP202, Design B, registered by `features/conversation-list`) and
 * `list.toggle-row` (Ctrl+Space, R5, `features/mail-actions`). An action
 * works on the list that holds the focus, else on the one mounted last.
 */

/** What a list offers its actions. */
export interface ListControl {
  /** The list's element (to find the focused one). */
  element: () => HTMLElement | null;
  /** → on the selected row (an expandable list only). */
  expand?: (() => void) | undefined;
  /** ← on the selected row (an expandable list only). */
  collapse?: (() => void) | undefined;
  /**
   * Ctrl+Space on the selected row: under "select all" leaves it out or
   * takes it back, otherwise picks it besides the other rows or takes it
   * out.
   */
  toggleRow: () => void;
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

/** The list the list actions act on, or `null` without a list. */
export function currentListControl(): ListControl | null {
  const active = typeof document === "undefined" ? null : document.activeElement;
  if (active) {
    const focused = controls.find((c) => c.element()?.contains(active) === true);
    if (focused) return focused;
  }
  return controls[controls.length - 1] ?? null;
}
