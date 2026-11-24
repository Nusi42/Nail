/**
 * Hooks of the calendar's views and shell (R7, ADR 0246): the zones a
 * widget shows (layout option, then the preferences, then the system's),
 * the size of an element, whether the window's layout shows the calendar
 * sidebar (else the calendar offers it as a drawer) and a touch screen.
 */
import { useContext, useLayoutEffect, useState, useSyncExternalStore, type RefObject } from "react";
import type { LayoutNode } from "../../api/generated";
import { LayoutContext } from "../../layout/LayoutContext";
import { useWindowWidth } from "../../layout/useWindowWidth";
import { optionString, type WidgetNode } from "../../layout/widgetRegistry";
import { useCalendarPreferences, viewZonesOf, type ViewZones } from "./prefs";
import { layoutShowsSidebar } from "./viewsModel";

/** The zones of a calendar widget: its `time-zone` / `secondary-zone` options, else the preferences. */
export function useCalendarZones(node?: WidgetNode): ViewZones {
  const prefs = useCalendarPreferences();
  return viewZonesOf(prefs, {
    zone: node ? optionString(node, "time-zone", "") : null,
    second: node ? optionString(node, "secondary-zone", "") : null,
  });
}

/** The content size of the element of `ref` in CSS pixels (0 until measured, or without `ResizeObserver`). */
export function useElementSize(ref: RefObject<HTMLElement | null>): {
  width: number;
  height: number;
} {
  const [size, setSize] = useState({ width: 0, height: 0 });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const measure = () => {
      const next = { width: Math.round(el.clientWidth), height: Math.round(el.clientHeight) };
      setSize((s) => (s.width === next.width && s.height === next.height ? s : next));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => {
      observer.disconnect();
    };
  }, [ref]);
  return size;
}

/** The root font size in CSS pixels (what `1rem` is; 16 when unknown). */
export function rootFontPx(): number {
  const size = Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
  return Number.isFinite(size) && size > 0 ? size : 16;
}

/** The content width of the element of `ref` in CSS pixels (see `useElementSize`). */
export function useElementWidth(ref: RefObject<HTMLElement | null>): number {
  return useElementSize(ref).width;
}

/** Whether the window showing `node` shows the calendar sidebar at its current width. */
export function useLayoutShowsSidebar(node: LayoutNode): boolean {
  const layout = useContext(LayoutContext);
  return layoutShowsSidebar(layout, node, useWindowWidth());
}

const COARSE = "(pointer: coarse)";

function subscribeCoarse(notify: () => void): () => void {
  if (typeof window.matchMedia !== "function") return () => undefined;
  const query = window.matchMedia(COARSE);
  query.addEventListener("change", notify);
  return () => {
    query.removeEventListener("change", notify);
  };
}

/** Whether the main pointer is a finger (touch screens: the floating New event button). */
export function useCoarsePointer(): boolean {
  return useSyncExternalStore(
    subscribeCoarse,
    () => typeof window.matchMedia === "function" && window.matchMedia(COARSE).matches,
    () => false,
  );
}
