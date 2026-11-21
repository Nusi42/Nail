/**
 * Unsaved changes survive closing the rules window (R2 design review,
 * item 4): when the window leaves the open windows (its close button,
 * Escape — the layout's `window.close` — or a command) while the editor has
 * unsaved changes, it stays open and the editor asks "Discard changes?"
 * (`pending` in `./store.ts`); "Discard changes" closes it for good.
 *
 * Like the composer's close confirmation (`features/composer/sessions.ts`),
 * the window is put back in the same store update, before React renders, so
 * the editor keeps its state.
 */
import { useEffect } from "react";
import type { NailClient } from "../../api/client";
import { useClient } from "../../state/queries";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";
import { rulesStore } from "./store";

/** The window with the rules (also the dialog's name). */
export const RULES_WINDOW = "rules";

/**
 * Keeps the rules window open while its rule has unsaved changes and asks
 * instead; returns the unsubscribe function.
 */
export function guardRulesWindow(ui: UiStore, client: NailClient): () => void {
  let closing = false;
  return ui.subscribe((next, prev) => {
    if (closing) return;
    if (!prev.windows.includes(RULES_WINDOW) || next.windows.includes(RULES_WINDOW)) return;
    const rules = rulesStore(client);
    if (!rules.getState().dirty) return;
    // Back on top, and ask.
    ui.setState({ windows: [...next.windows, RULES_WINDOW] });
    rules.getState().guard({
      kind: "close",
      close: () => {
        closing = true;
        try {
          ui.getState().closeWindow(RULES_WINDOW);
        } finally {
          closing = false;
        }
      },
    });
  });
}

/** Background component: the guard for the app's UI store. */
export function RulesCloseGuard() {
  const ui = useUiStoreApi();
  const client = useClient();
  useEffect(() => guardRulesWindow(ui, client), [ui, client]);
  return null;
}
