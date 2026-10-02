/**
 * Names this feature registers: the dialog (window title
 * `window-command-palette` in `palette.ftl`) and the action ids. The key
 * bindings (default Ctrl+K and Ctrl+Shift+P) live in `layout.kdl` like
 * every other binding.
 */

/** Dialog: the command palette. */
export const PALETTE_DIALOG = "command-palette";

/** Action: opens the command palette (closes it when it is open). */
export const PALETTE_OPEN_ACTION = "palette.open";
/** Action: forgets the recently used actions. */
export const PALETTE_CLEAR_RECENT_ACTION = "palette.clear-recent";
