/**
 * Plain data of the Customize tab's editors (no React), shared with the
 * catalog coverage test.
 */

/**
 * Every key context the UI has: each `data-key-context` an element sets
 * (`global` applies when no context matches). The key-binding editor offers
 * these, each with a `customize-keys-context-<name>` label; contexts a
 * user's layout invents are shown verbatim. The catalog coverage test
 * checks the UI sources, `defaults/layout.kdl` and the plug-in manifest's
 * contexts against this list, so a new context cannot ship without a label.
 */
export const KEY_CONTEXTS = [
  "global",
  "list",
  "reader",
  "composer",
  "search",
  "folders",
  "calendar",
  "contacts",
  "tasks",
  "window",
  "menu",
  "plugin-dialog",
  "plugin-veto",
] as const;

/** One of the UI's own key contexts. */
export type KeyContext = (typeof KEY_CONTEXTS)[number];

/** Whether `context` is one of the UI's own [`KEY_CONTEXTS`]. */
export function isKnownKeyContext(context: string): context is KeyContext {
  return (KEY_CONTEXTS as readonly string[]).includes(context);
}
