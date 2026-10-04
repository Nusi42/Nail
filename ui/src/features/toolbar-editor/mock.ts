/**
 * The mock core's side of the toolbar customizer's patches (M5d, ADR 0131):
 * `move-toolbar-item`, `add-toolbar-item`, `remove-toolbar-item`, applied to
 * the mock's resolved layout by the customization mock's `UpdateLayout`
 * (`../customization/mock.ts`), which also records the undo entry and
 * announces `ConfigChanged`.
 *
 * A toolbar the layout does not have, an empty slot or an item that cannot
 * go where it is sent answers an error message (the customization mock
 * answers it as `invalid_request`, like the core's
 * `edit-issue-toolbar-slot-invalid`).
 */
import type { LayoutPatch, ResolvedLayout } from "../../api/generated";
import { ToolbarEditError, applyToolbarEdit } from "./model";

/** The ops this module applies. */
export const TOOLBAR_EDIT_OPS = [
  "move-toolbar-item",
  "add-toolbar-item",
  "remove-toolbar-item",
] as const satisfies readonly LayoutPatch["op"][];

export type ToolbarEditPatch = Extract<LayoutPatch, { op: (typeof TOOLBAR_EDIT_OPS)[number] }>;

export function isToolbarEditPatch(patch: LayoutPatch): patch is ToolbarEditPatch {
  return (TOOLBAR_EDIT_OPS as readonly string[]).includes(patch.op);
}

/**
 * Applies one patch to `layout` (in place); answers an error message, or
 * `null` when it was applied.
 */
export function applyToolbarEditPatch(
  layout: ResolvedLayout,
  patch: ToolbarEditPatch,
): string | null {
  if (!Object.hasOwn(layout.toolbars, patch.toolbar)) return `no toolbar ${patch.toolbar}`;
  try {
    layout.toolbars[patch.toolbar] = applyToolbarEdit(layout.toolbars[patch.toolbar] ?? [], patch);
    return null;
  } catch (error) {
    if (error instanceof ToolbarEditError) return error.message;
    throw error;
  }
}
