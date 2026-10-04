/**
 * The mock core's side of the layout editor's patches (M5d, ADR 0131):
 * `move-pane`, `swap-panes`, `add-widget`, `remove-node`, `set-hidden`,
 * `apply-preset`, `reset-window`, applied to the mock's resolved layout by
 * the customization mock's `UpdateLayout` (`../customization/mock.ts`),
 * which also records the undo entry and announces `ConfigChanged`.
 *
 * Slot of WP181: until it lands every patch answers
 * `MOCK_NOT_READY` (the customization mock turns it into
 * `unsupported` / `edit-issue-not-ready`, like the contract's core).
 */
import type { LayoutPatch, ResolvedLayout } from "../../api/generated";
import { MOCK_NOT_READY } from "../../api/editHistoryMock";

/** The ops this module applies. */
export const LAYOUT_EDIT_OPS = [
  "move-pane",
  "swap-panes",
  "add-widget",
  "remove-node",
  "set-hidden",
  "apply-preset",
  "reset-window",
] as const satisfies readonly LayoutPatch["op"][];

export type LayoutEditPatch = Extract<LayoutPatch, { op: (typeof LAYOUT_EDIT_OPS)[number] }>;

export function isLayoutEditPatch(patch: LayoutPatch): patch is LayoutEditPatch {
  return (LAYOUT_EDIT_OPS as readonly string[]).includes(patch.op);
}

/**
 * Applies one patch to `layout` (in place); answers an error message, or
 * `null` when it was applied.
 */
export const applyLayoutEditPatch: (
  layout: ResolvedLayout,
  patch: LayoutEditPatch,
) => string | null = () => MOCK_NOT_READY;
