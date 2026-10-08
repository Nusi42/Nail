/**
 * Pointer dragging in customize mode (M5d WP181): a pane's drag handle or a
 * palette widget is pressed and moved; the drop targets of every pane show
 * (the same buttons the keyboard reaches), the one under the pointer is
 * highlighted, and releasing over it drops there (by clicking that target,
 * so pointer and keyboard share one code path). Releasing elsewhere
 * cancels; a press without movement is a click (pick up / put back).
 */
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  type PointerEvent as ReactPointerEvent,
} from "react";
import type { DropZone, Moving } from "./model";
import { editorStore } from "./store";
import { DRAG_THRESHOLD } from "../windows/geometry";

/** Pixels the pointer must move before a press becomes a drag. */

/** The part of a drop target button. */
export const TARGET_PART = "layout-editor.target";

/** The drop target under a point, if any. */
export function targetAt(x: number, y: number): { pane: string; zone: DropZone } | null {
  // jsdom has no `elementFromPoint`.
  const hit =
    typeof document.elementFromPoint === "function" ? document.elementFromPoint(x, y) : null;
  const button = hit?.closest<HTMLElement>(`[data-part="${TARGET_PART}"]`);
  const pane = button?.dataset.pane;
  const zone = button?.dataset.zone;
  if (pane === undefined || zone === undefined) return null;
  return { pane, zone: zone as DropZone };
}

/** The drop target button of a pane (`refKey`: kebab names, digits and `/`) and zone. */
export function targetButton(pane: string, zone: DropZone): HTMLButtonElement | null {
  return document.querySelector<HTMLButtonElement>(
    `[data-part="${TARGET_PART}"][data-pane="${pane}"][data-zone="${zone}"]`,
  );
}

interface Press {
  pointerId: number;
  x: number;
  y: number;
}

/**
 * Pointer handlers for a drag source. `moving` describes what it drags;
 * `isDragClick()` tells the source's click handler that the click ended a
 * drag (and must be ignored).
 */
export function useDragSource(moving: () => Moving): {
  onPointerDown: (event: ReactPointerEvent<HTMLElement>) => void;
  isDragClick: () => boolean;
} {
  const press = useRef<Press | null>(null);
  const dragged = useRef(false);
  const cleanup = useRef<(() => void) | null>(null);
  const describe = useRef(moving);
  useLayoutEffect(() => {
    describe.current = moving;
  });

  useEffect(
    () => () => {
      cleanup.current?.();
    },
    [],
  );

  const onPointerDown = useCallback((event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0 || editorStore.getState().busy) return;
    press.current = { pointerId: event.pointerId, x: event.clientX, y: event.clientY };
    dragged.current = false;
    function onMove(e: PointerEvent) {
      const start = press.current;
      if (start?.pointerId !== e.pointerId) return;
      if (!editorStore.getState().dragging) {
        if (Math.hypot(e.clientX - start.x, e.clientY - start.y) < DRAG_THRESHOLD) return;
        editorStore.setState({
          moving: describe.current(),
          dragging: true,
          hover: null,
          message: null,
        });
      }
      const hover = targetAt(e.clientX, e.clientY);
      const old = editorStore.getState().hover;
      if (hover?.pane !== old?.pane || hover?.zone !== old?.zone) editorStore.setState({ hover });
    }
    function stop() {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", finish);
      window.removeEventListener("pointercancel", finish);
      cleanup.current = null;
    }
    function finish(e: PointerEvent) {
      if (press.current?.pointerId !== e.pointerId) return;
      press.current = null;
      stop();
      if (!editorStore.getState().dragging) return;
      dragged.current = true;
      const hover = e.type === "pointercancel" ? null : targetAt(e.clientX, e.clientY);
      const button = hover ? targetButton(hover.pane, hover.zone) : null;
      if (button) {
        button.click();
      } else {
        editorStore.setState({ moving: null, dragging: false, hover: null });
      }
      // The click that may follow the release belongs to the drag.
      setTimeout(() => {
        dragged.current = false;
      }, 0);
    }
    cleanup.current?.();
    cleanup.current = stop;
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", finish);
    window.addEventListener("pointercancel", finish);
  }, []);

  const isDragClick = useCallback(() => dragged.current, []);
  return { onPointerDown, isDragClick };
}
