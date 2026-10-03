/**
 * Commands for the window manager from outside the window layer (actions,
 * key bindings): the layer of a UI store registers itself here while it is
 * mounted (`setWindowCommandHandler`), actions send it commands
 * (`runWindowCommand`). Keyed by store, so several apps (tests) never mix.
 */
import type { UiStore } from "../../state/uiStore";

export type WindowCommand =
  /** Ctrl+F6: the next window (raised and focused), or back to the main window. */
  | { type: "cycle" }
  /** Ctrl+Shift+F6: focus back to the main window. */
  | { type: "main" }
  /** Maximize / restore the active window. */
  | { type: "maximize" }
  /** Remembered positions were forgotten: open windows go back to their default places. */
  | { type: "reset" };

type Handler = (command: WindowCommand) => void;

const handlers = new WeakMap<UiStore, Handler>();

/** Registers (or with `null` removes) the window layer of `store`. */
export function setWindowCommandHandler(store: UiStore, handler: Handler | null): void {
  if (handler === null) handlers.delete(store);
  else handlers.set(store, handler);
}

/** Sends a command to the window layer of `store`; `false` when none is mounted. */
export function runWindowCommand(store: UiStore, command: WindowCommand): boolean {
  const handler = handlers.get(store);
  if (handler === undefined) return false;
  handler(command);
  return true;
}
