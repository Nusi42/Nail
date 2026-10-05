/**
 * The time grid's opening scroll position (P1 WP192): set when the view is
 * shown and set again whenever the scroll area changes size, until the user
 * scrolls it themselves. The first position is computed before the layout
 * has settled (pane sizes arrive as geometry rules after the first render,
 * and a narrow calendar's toolbar takes two rows), so a single assignment
 * could land an hour off; a resized window shows the chosen hour again too.
 */

/** Events that mean the user moves the grid themselves. */
const USER_EVENTS = ["wheel", "pointerdown", "keydown", "touchstart"] as const;

/**
 * Applies `position()` to `el.scrollTop` now and on every size change of
 * `el` until a user event on it; returns the cleanup.
 */
export function keepInitialScroll(el: HTMLElement, position: () => number): () => void {
  let touched = false;
  const apply = () => {
    if (touched) return;
    const top = position();
    if (el.scrollTop !== top) el.scrollTop = top;
  };
  const touch = () => {
    touched = true;
  };
  apply();
  for (const type of USER_EVENTS) el.addEventListener(type, touch, { passive: true });
  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(apply);
  observer?.observe(el);
  return () => {
    observer?.disconnect();
    for (const type of USER_EVENTS) el.removeEventListener(type, touch);
  };
}
