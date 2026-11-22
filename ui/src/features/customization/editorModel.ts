/**
 * Plain data of the Customize tab's editors (no React), shared with the
 * catalog coverage test.
 */
import type { KeyBinding, LayoutPatch } from "../../api/generated";

/**
 * Every key context the UI has: each `data-key-context` an element sets
 * (`global` applies when no context matches). The key-binding editor offers
 * these, each with a `customize-keys-context-<name>` label; contexts a
 * user's layout invents are shown verbatim. The catalog coverage test
 * checks the UI sources, `defaults/layout.kdl` and the plug-in manifest's
 * contexts (the contract's `KEY_CONTEXTS`, a subset of this list) against
 * it, so a new context cannot ship without a label: pinned by
 * `ui/src/i18n/coverage.test.ts`.
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

/** A key-binding change waiting for the settings page's Save (R6, ADR 0239). */
export type KeyBindingChange = Extract<
  LayoutPatch,
  { op: "add-key-binding" | "remove-key-binding" | "reset-key-binding" }
>;

/** A shown binding and the unsaved change that made it (`null`: as saved). */
export interface PreviewBinding {
  binding: KeyBinding;
  change: "add" | "reset" | null;
}

/** Whether `binding` is the binding of `key` in `context`. */
const isBinding = (binding: KeyBinding, change: { context: string; key: string }) =>
  binding.context === change.context && binding.key === change.key;

/**
 * The key bindings as they will be once `changes` are saved: an added one
 * replaces the binding of its key in its context (as the core does), a
 * removed one goes, a reset one stays marked (the core restores its
 * default on Save).
 */
export function previewKeyBindings(
  saved: readonly KeyBinding[],
  changes: readonly KeyBindingChange[],
): PreviewBinding[] {
  let shown: PreviewBinding[] = saved.map((binding) => ({ binding, change: null }));
  for (const change of changes) {
    const others = shown.filter((p) => !isBinding(p.binding, change));
    switch (change.op) {
      case "add-key-binding":
        shown = [
          ...others,
          {
            binding: {
              context: change.context,
              key: change.key,
              action: change.action,
              args: change.args,
            },
            change: "add",
          },
        ];
        break;
      case "remove-key-binding":
        shown = others;
        break;
      case "reset-key-binding":
        shown = shown.map((p) => (isBinding(p.binding, change) ? { ...p, change: "reset" } : p));
        break;
    }
  }
  return shown;
}
