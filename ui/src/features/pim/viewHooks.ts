/**
 * Hooks of the calendar's views and shell (R7, ADR 0246): the zones a
 * widget shows (layout option, then the preferences, then the system's),
 * the width of an element, whether the window's layout shows the calendar
 * sidebar (else the calendar offers it as a drawer), a touch screen, and
 * the calendar quick create adds to.
 */
import { useContext, useLayoutEffect, useState, useSyncExternalStore, type RefObject } from "react";
import type { Calendar, LayoutNode } from "../../api/generated";
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

/** The content width of the element of `ref` in CSS pixels (0 until measured, or without `ResizeObserver`). */
export function useElementWidth(ref: RefObject<HTMLElement | null>): number {
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const measure = () => {
      setWidth(Math.round(el.clientWidth));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => {
      observer.disconnect();
    };
  }, [ref]);
  return width;
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

/**
 * The calendar a quick-created event goes to: a writable event calendar of
 * this device first, else the first writable one (the event editor's
 * choice for a new event without a calendar).
 */
export function defaultEventCalendar(calendars: readonly Calendar[]): Calendar | null {
  const writable = calendars.filter((c) => !c.read_only && c.components.includes("event"));
  return writable.find((c) => c.source_id === "local") ?? writable[0] ?? null;
}
