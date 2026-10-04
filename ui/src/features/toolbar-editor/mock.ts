/**
 * The mock core's side of the toolbar customizer's patches (M5d, ADR 0131):
 * `move-toolbar-item`, `add-toolbar-item`, `remove-toolbar-item`, applied to
 * the mock's resolved layout by the customization mock's `UpdateLayout`
 * (`../customization/mock.ts`), which also records the undo entry and
 * announces `ConfigChanged`.
 *
 * Slot of WP182: until it lands every patch answers `MOCK_NOT_READY` (the
 * customization mock turns it into `unsupported` / `edit-issue-not-ready`,
 * like the contract's core).
 */
import type { LayoutPatch, ResolvedLayout } from "../../api/generated";
import { MOCK_NOT_READY } from "../../api/editHistoryMock";

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
export const applyToolbarEditPatch: (
  layout: ResolvedLayout,
  patch: ToolbarEditPatch,
) => string | null = () => MOCK_NOT_READY;
