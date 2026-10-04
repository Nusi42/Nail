/**
 * The content of a pop-out window (route `#/window/<id>`, M5c ADR 0116):
 * one window — a layout window or a feature dialog — filling the OS window,
 * inside its `WindowInstanceContext`. `App.tsx` renders it instead of the
 * main window when the page was opened at a pop-out route.
 *
 * - A header with the window's title and "Return to main window"
 *   (`ReturnPopOut`: the OS window closes, the window opens in the page).
 * - The document title follows the window's title — the desktop shell
 *   copies it to the OS window and its taskbar entry; content names it with
 *   `usePopOutTitle` (the composer's subject).
 * - The window is open in this webview's own UI store, so readers of the
 *   focused window find it; when the content closes it (Send, Cancel,
 *   Close), the OS window closes (`ClosePopOut`).
 * - The context the window was opened with is read with `usePopOutContext`.
 * - Windows the content opens (a composer's "Discard?" / "Close?"
 *   questions, plug-in dialogs) show in this page's own window layer, with
 *   the pop-out as the main window behind them.
 * - Closing the OS window (its close button, Alt+F4) closes the content the
 *   way its close button in the page does (`POP_OUT_CLOSE_EVENT` from the
 *   shell): a composer with unsaved changes asks first, and the OS window
 *   closes once the content is closed.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import type { ResolvedLayout } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { WindowView } from "../../layout/LayoutRenderer";
import { WindowLayer } from "../../layout/WindowLayer";
import { useDialogRegistry } from "../../layout/dialogRegistry";
import { useWindowSpecs } from "../../layout/windowSpecs";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { usePopOutActions } from "../../state/windowState";
import {
  POP_OUT_CLOSE_EVENT,
  WindowInstanceContext,
  resolveWindowSpec,
  windowKind,
} from "../../state/windows";
import { Icon } from "../../widgets/Icon";
import { PopOutTitleContext } from "./hooks";
import { errorMessageId, preparerOf, returnPopOut, windowTitleId } from "./model";

export function PopOutHost({ layout, window }: { layout: ResolvedLayout; window: string }) {
  const t = useT();
  const dialogs = useDialogRegistry();
  const store = useUiStoreApi();
  const specs = useWindowSpecs();
  const client = useClient();
  const queryClient = useQueryClient();
  const { close } = usePopOutActions();
  const closeRef = useRef(close);
  useEffect(() => {
    closeRef.current = close;
  });
  const [customTitle, setCustomTitle] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const kind = windowKind(window);
  const title = customTitle ?? t(windowTitleId(window));

  useEffect(() => {
    document.title = title;
  }, [title]);

  // Open in this webview's store; closed by the content → close the OS window.
  // Judged once the store settled: a store listener may see a change that
  // another listener takes back in the same update (a composer with unsaved
  // changes stays open and asks).
  useEffect(() => {
    store.getState().openWindow(window);
    let closed = false;
    let pending = false;
    const unsubscribe = store.subscribe(() => {
      if (pending || closed) return;
      pending = true;
      queueMicrotask(() => {
        pending = false;
        if (closed || store.getState().windows.includes(window)) return;
        closed = true;
        void closeRef.current(window).catch(() => undefined);
      });
    });
    return () => {
      closed = true;
      unsubscribe();
    };
  }, [store, window]);

  // The user closes the OS window: close the content as its close button
  // would (an open question is answered first; it stays in front).
  useEffect(() => {
    const onCloseRequested = () => {
      const open = store.getState().windows;
      const asking = open.some((id) => id !== window && resolveWindowSpec(id, layout, specs).modal);
      if (asking) return;
      store.getState().closeWindow(window);
    };
    globalThis.addEventListener(POP_OUT_CLOSE_EVENT, onCloseRequested);
    return () => {
      globalThis.removeEventListener(POP_OUT_CLOSE_EVENT, onCloseRequested);
    };
  }, [store, window, layout, specs]);

  const closeContent = () => {
    store.getState().closeWindow(window);
  };
  // A layout window wins; a feature dialog fills in for a name the layout lacks.
  const Dialog = Object.hasOwn(layout.windows, kind) ? undefined : dialogs[kind];
  const titleId = `nail-pop-out-${window.replace(":", "-")}-title`;
  return (
    <PopOutTitleContext.Provider value={setCustomTitle}>
      <WindowInstanceContext.Provider value={window}>
        <section
          data-part="pop-out"
          data-window={window}
          data-key-context="window"
          aria-labelledby={titleId}
        >
          <div data-part="pop-out.header">
            <h1 data-part="pop-out.title" id={titleId}>
              {title}
            </h1>
            {error !== null && (
              <p data-part="pop-out.error" role="alert">
                {t(error)}
              </p>
            )}
            <button
              type="button"
              data-part="pop-out.return"
              title={t("popout-return-hint")}
              onClick={() => {
                setError(null);
                // What only this page holds is saved first (the composer's
                // draft), like before popping out.
                const prepare = preparerOf(window);
                Promise.resolve(prepare?.(window, { client, store, queryClient }))
                  .then(() => returnPopOut({ client, queryClient }, window))
                  .catch((e: unknown) => {
                    setError(errorMessageId(e));
                  });
              }}
            >
              <Icon name="app-window" part="pop-out.return.icon" />
              <span data-part="pop-out.return.label">{t("popout-return")}</span>
            </button>
          </div>
          <div data-part="pop-out.body">
            {Dialog ? (
              <Dialog name={window} close={closeContent} />
            ) : (
              <WindowView layout={layout} name={kind} />
            )}
          </div>
        </section>
      </WindowInstanceContext.Provider>
      <WindowLayer layout={layout} main={window} />
    </PopOutTitleContext.Provider>
  );
}
