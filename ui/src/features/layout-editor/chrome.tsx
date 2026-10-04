/**
 * The layout chrome of customize mode (M5d WP181): for the window being
 * customized, every node goes through `PaneChrome` (pane headers, drop
 * targets) and the window through `EditorFrame` (banner and palette);
 * hidden nodes render so they can be shown again. Registered for the
 * window by `startEditing` (`./store.ts`).
 */
import type { LayoutChrome } from "../../layout/LayoutChrome";
import { EditorFrame } from "./EditorFrame";
import { PaneChrome } from "./Pane";

/** The chrome of window `window` in customize mode. */
export function editorChrome(window: string): LayoutChrome {
  return {
    window,
    showHidden: true,
    renderNode: (node, ref, content) =>
      ref.window === window ? (
        <PaneChrome node={node} nodeRef={ref}>
          {content}
        </PaneChrome>
      ) : (
        content
      ),
    renderWindow: (name, breakpoint, content) =>
      name === window ? (
        <EditorFrame window={name} breakpoint={breakpoint}>
          {content}
        </EditorFrame>
      ) : (
        content
      ),
  };
}
