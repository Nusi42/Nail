/**
 * The calendar toolbar's fallback for narrow panes (M5b review): when the
 * segmented view switch does not fit beside the range (the switch scrolls,
 * the range is cut or the bar itself overflows), the widget shows the view
 * menu instead, and goes back to the segmented switch once the bar is as
 * wide as the segmented row needed.
 */
import { useLayoutEffect, useRef, useState, type RefObject } from "react";

/** Pixels the bar's content needs beyond its width (0 when it fits). */
export function toolbarOverflow(bar: HTMLElement): number {
  const over = (el: Element | null) => (el ? Math.max(0, el.scrollWidth - el.clientWidth) : 0);
  return (
    over(bar) +
    over(bar.querySelector('[data-part="segmented"]')) +
    over(bar.querySelector('[data-part="calendar.title"]'))
  );
}

/**
 * Whether the bar shows the view menu instead of the segmented switch.
 * `enabled` is false when the layout asks for the menu anyway or there is
 * no switch; `watch` re-checks when the bar's content changes (the range
 * text, the view).
 */
export function useNarrowViewSwitch(
  bar: RefObject<HTMLElement | null>,
  enabled: boolean,
  watch: string,
): boolean {
  const [narrow, setNarrow] = useState(false);
  const state = useRef({ narrow: false, needed: 0 });

  useLayoutEffect(() => {
    const el = bar.current;
    if (!el || !enabled) return;
    const check = () => {
      const width = el.clientWidth;
      if (width <= 0) return;
      if (!state.current.narrow) {
        const overflow = toolbarOverflow(el);
        if (overflow > 1) {
          state.current = { narrow: true, needed: width + overflow };
          setNarrow(true);
        }
      } else if (width >= state.current.needed) {
        state.current = { narrow: false, needed: 0 };
        setNarrow(false);
      }
    };
    check();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(check);
    observer.observe(el);
    return () => observer.disconnect();
  }, [bar, enabled, watch, narrow]);

  return enabled && narrow;
}
