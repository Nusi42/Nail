/**
 * Arrow-key focus movement inside a group of buttons (a toolbar, the
 * navigation rail): the previous / next key moves to the neighbour
 * (wrapping), Home / End to the first / last; modified keys are left alone.
 */
import type { KeyboardEvent } from "react";

export interface RovingKeys {
  previous: string;
  next: string;
}

/** Arrow keys of a horizontal group (a toolbar). */
export const HORIZONTAL: RovingKeys = { previous: "ArrowLeft", next: "ArrowRight" };
/** Arrow keys of a vertical group (the navigation rail). */
export const VERTICAL: RovingKeys = { previous: "ArrowUp", next: "ArrowDown" };

/** Moves the focus among the elements `selector` finds in the group. */
export function moveRovingFocus(
  event: KeyboardEvent<HTMLElement>,
  selector: string,
  keys: RovingKeys,
): void {
  const handled = [keys.previous, keys.next, "Home", "End"];
  if (!handled.includes(event.key) || event.altKey || event.ctrlKey || event.metaKey) return;
  const buttons = [...event.currentTarget.querySelectorAll<HTMLElement>(selector)];
  if (buttons.length === 0) return;
  const at = buttons.indexOf(event.target as HTMLElement);
  let next: number;
  if (event.key === "Home") next = 0;
  else if (event.key === "End") next = buttons.length - 1;
  else if (at < 0) next = 0;
  else next = (at + (event.key === keys.next ? 1 : -1) + buttons.length) % buttons.length;
  event.preventDefault();
  buttons[next]?.focus();
}
