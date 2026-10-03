/**
 * The content of a pop-out window (route `#/window/<id>`, M5c ADR 0116):
 * one window — a layout window or a feature dialog — filling the OS window,
 * inside its `WindowInstanceContext`. `App.tsx` renders it instead of the
 * main window when the page was opened at a pop-out route. Contract stage:
 * the plain content; WP173 adds the title, "Return to main window" and the
 * hand-over of the window's context (`PopOut.context`).
 */
import type { ResolvedLayout } from "../../api/generated";
import { WindowView } from "../../layout/LayoutRenderer";
import { useDialogRegistry } from "../../layout/dialogRegistry";
import { usePopOutActions } from "../../state/windowState";
import { WindowInstanceContext, windowKind } from "../../state/windows";

export function PopOutHost({ layout, window }: { layout: ResolvedLayout; window: string }) {
  const dialogs = useDialogRegistry();
  const { close } = usePopOutActions();
  const kind = windowKind(window);
  // A layout window wins; a feature dialog fills in for a name the layout lacks.
  const Dialog = Object.hasOwn(layout.windows, kind) ? undefined : dialogs[kind];
  return (
    <WindowInstanceContext.Provider value={window}>
      <div data-part="pop-out" data-window={window}>
        {Dialog ? (
          <Dialog name={window} close={() => void close(window).catch(() => undefined)} />
        ) : (
          <WindowView layout={layout} name={kind} />
        )}
      </div>
    </WindowInstanceContext.Provider>
  );
}
