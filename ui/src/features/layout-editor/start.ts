/**
 * Entering customize mode (M5d WP181): `layout.customize` (palette, menus)
 * customizes the window of the view shown now — or, when the main window
 * hosts no views (a pre-rail layout), the main window; the settings card
 * names a window, whose view is shown first (or whose window is opened).
 */
import type { ResolvedLayout } from "../../api/generated";
import { currentView, hostsViews, viewDef, viewOfWindow } from "../../layout/views";
import type { UiStore } from "../../state/uiStore";
import { startEditing } from "./store";

/** The window `layout.customize` customizes now. */
export function customizeTarget(layout: ResolvedLayout, store: UiStore): string | null {
  if (!hostsViews(layout)) return Object.hasOwn(layout.windows, "main") ? "main" : null;
  const view = currentView(layout, store.getState().activeView);
  const window = view === null ? null : (viewDef(layout, view)?.window ?? null);
  return window !== null && Object.hasOwn(layout.windows, window) ? window : null;
}

/** Shows `window` (its view, or the window) and starts customizing it. */
export function startCustomizing(
  store: UiStore,
  layout: ResolvedLayout,
  window: string | null = customizeTarget(layout, store),
): boolean {
  if (window === null || !Object.hasOwn(layout.windows, window)) return false;
  const view = viewOfWindow(layout, window);
  if (window !== "main") {
    if (hostsViews(layout) && view !== null) store.getState().setActiveView(view);
    else if (!store.getState().windows.includes(window)) store.getState().openWindow(window);
  }
  startEditing(window);
  return true;
}
