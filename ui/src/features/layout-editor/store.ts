/**
 * The customize-layout session (M5d WP181): which window is being edited,
 * what is picked up (keyboard) or dragged (pointer), the drop target under
 * the pointer and the last status message. One session at a time for the
 * page; starting it registers the editor's chrome for the window
 * (`../../layout/LayoutChrome.ts`), so that window renders with pane
 * headers, the banner and the palette.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { LocalizedMessage } from "../../api/generated";
import { setWindowChrome, type LayoutChrome } from "../../layout/LayoutChrome";
import type { DropZone, Moving } from "./model";

/** A status line of the editor (announced politely). */
export interface EditorMessage {
  kind: "status" | "error";
  text: LocalizedMessage;
  /** The error was a `conflict` of Undo / Redo: offer to force it. */
  forceStep?: "undo" | "redo";
}

export interface EditorState {
  /** The window being customized, `null` outside customize mode. */
  window: string | null;
  /** What is picked up or dragged. */
  moving: Moving | null;
  /** A pointer drag is under way (`moving` holds what is dragged). */
  dragging: boolean;
  /** The drop target under the pointer: a pane (`refKey`) and zone. */
  hover: { pane: string; zone: DropZone } | null;
  /** An edit is being written. */
  busy: boolean;
  message: EditorMessage | null;
  /** The pane (node id) to focus after the next layout change. */
  focusPane: string | null;
}

const INITIAL: EditorState = {
  window: null,
  moving: null,
  dragging: false,
  hover: null,
  busy: false,
  message: null,
  focusPane: null,
};

export const editorStore = createStore<EditorState>(() => ({ ...INITIAL }));

/** Selects from the session. */
export function useEditor<T>(selector: (state: EditorState) => T): T {
  return useStore(editorStore, selector);
}

/** Builds the chrome of a window (set by `./chrome.tsx`, kept apart to avoid an import cycle). */
let chromeFactory: ((window: string) => LayoutChrome) | null = null;

export function setChromeFactory(factory: (window: string) => LayoutChrome): void {
  chromeFactory = factory;
}

/** The element focused before customize mode started (focused again by Done). */
let returnFocus: HTMLElement | null = null;

/** Starts customize mode for `window` (ends a session of another window). */
export function startEditing(window: string): void {
  const current = editorStore.getState().window;
  if (current === window) return;
  if (current !== null) setWindowChrome(current, null);
  else returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  editorStore.setState({ ...INITIAL, window });
  if (chromeFactory) setWindowChrome(window, chromeFactory(window));
}

/** Ends customize mode ("Done"). */
export function stopEditing(): void {
  const { window } = editorStore.getState();
  if (window === null) return;
  setWindowChrome(window, null);
  editorStore.setState({ ...INITIAL });
  const target = returnFocus;
  returnFocus = null;
  if (target?.isConnected) target.focus();
}

/** Picks up (or puts back, with `null`) what the keyboard / a click moves. */
export function setMoving(moving: Moving | null): void {
  editorStore.setState({ moving, dragging: false, hover: null, message: null });
}

export function setMessage(message: EditorMessage | null): void {
  editorStore.setState({ message });
}

/** Resets the session (tests). */
export function resetEditor(): void {
  const { window } = editorStore.getState();
  if (window !== null) setWindowChrome(window, null);
  returnFocus = null;
  editorStore.setState({ ...INITIAL });
}
