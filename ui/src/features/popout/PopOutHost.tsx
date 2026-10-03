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
 */
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import type { ResolvedLayout } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { WindowView } from "../../layout/LayoutRenderer";
import { useDialogRegistry } from "../../layout/dialogRegistry";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { usePopOutActions } from "../../state/windowState";
import { WindowInstanceContext, windowKind } from "../../state/windows";
import { Icon } from "../../widgets/Icon";
import { PopOutTitleContext } from "./hooks";
import { errorMessageId, preparerOf, returnPopOut, windowTitleId } from "./model";

export function PopOutHost({ layout, window }: { layout: ResolvedLayout; window: string }) {
  const t = useT();
  const dialogs = useDialogRegistry();
  const store = useUiStoreApi();
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
  useEffect(() => {
    store.getState().openWindow(window);
    let open = true;
    return store.subscribe((state) => {
      const now = state.windows.includes(window);
      if (open && !now) void closeRef.current(window).catch(() => undefined);
      open = now;
    });
  }, [store, window]);

  const closeContent = () => store.getState().closeWindow(window);
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
                  .catch((e: unknown) => setError(errorMessageId(e)));
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
    </PopOutTitleContext.Provider>
  );
}
