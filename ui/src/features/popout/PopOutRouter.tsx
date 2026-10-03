/**
 * Keeps popped-out windows out of the main page (M5c WP173; an effect,
 * renders nothing). When the main page opens a window (`openWindow`):
 *
 * - that window is popped out already → its OS window comes to the front
 *   instead (`FocusPopOut`) and the page does not show it twice;
 * - its kind was last shown popped out (`WindowState.popped_out`) and may
 *   pop out → it opens as an OS window again.
 *
 * Only in the main page of the desktop app; a window that failed to pop
 * out stays in the page (and is not tried again until it is reopened).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useContext, useEffect, useRef } from "react";
import { useT } from "../../i18n/I18nProvider";
import { LayoutContext } from "../../layout/LayoutContext";
import { useWindowSpecs } from "../../layout/windowSpecs";
import { useClient } from "../../state/queries";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { usePopOuts, useWindowStates } from "../../state/windowState";
import { resolveWindowSpec, windowKind } from "../../state/windows";
import { usePopOutsAvailable } from "./hooks";
import { focusPopOut, isPopOutPage, isPoppedOut, popOutWindow, windowTitleId } from "./model";

export function PopOutRouter() {
  const available = usePopOutsAvailable();
  if (!available || isPopOutPage()) return null;
  return <Router />;
}

function Router() {
  const t = useT();
  const client = useClient();
  const store = useUiStoreApi();
  const queryClient = useQueryClient();
  const layout = useContext(LayoutContext);
  // From the app's context, not an import of the registry (which imports
  // this feature: an import cycle evaluated at start-up).
  const specs = useWindowSpecs();
  const windows = useUi((s) => s.windows);
  const popOuts = usePopOuts().data;
  const states = useWindowStates().data;
  const seen = useRef<ReadonlySet<string>>(new Set());

  useEffect(() => {
    // Wait for both lists, so a window is judged once with full knowledge.
    if (popOuts === undefined || states === undefined) return;
    const before = seen.current;
    seen.current = new Set(windows);
    for (const window of windows) {
      if (before.has(window)) continue;
      const ctx = { client, store, queryClient };
      if (isPoppedOut(popOuts, window)) {
        store.getState().closeWindow(window);
        void focusPopOut(ctx, window).catch(() => undefined);
        continue;
      }
      const kind = windowKind(window);
      const state = Object.hasOwn(states, kind) ? states[kind] : undefined;
      if (state?.popped_out && resolveWindowSpec(window, layout, specs).pop_out) {
        void popOutWindow(ctx, window, t(windowTitleId(window))).catch(() => undefined);
      }
    }
  }, [windows, popOuts, states, client, store, queryClient, layout, specs, t]);

  return null;
}
