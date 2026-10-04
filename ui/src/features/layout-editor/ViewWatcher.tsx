/**
 * Ends customize mode when the user switches to another view (M5d review,
 * ADR 0138): the edited view stays mounted while hidden, and a session
 * nobody sees made the next "Customize layout" end it instead of starting
 * one for the view on screen.
 */
import { useEffect } from "react";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { viewOfWindow } from "../../layout/views";
import { useUi } from "../../state/uiStore";
import { stopEditing, useEditor } from "./store";

export function CustomizeViewWatcher() {
  const layout = useLayoutConfig();
  const activeView = useUi((s) => s.activeView);
  const window = useEditor((s) => s.window);
  useEffect(() => {
    if (window === null || activeView === null) return;
    const view = viewOfWindow(layout, window);
    if (view !== null && view !== activeView) stopEditing();
  }, [layout, activeView, window]);
  return null;
}
