/**
 * The move window (`mail.move`, `v`) moves what was the target when it
 * opened: the selected conversation (its messages in the listed folder),
 * the picked rows, or a card's message (P3, ADR 0164).
 */
import { currentTargets, type ActionTargets } from "../../actions/mailTargets";
import type { FolderId } from "../../api/generated";
import type { WriteContext } from "../../state/mailOps";
import type { UiStore } from "../../state/uiStore";
import { runMailAction } from "./run";

const pending = new WeakMap<UiStore, ActionTargets>();

/** Remembers the targets of the move window that opens now. */
export function setPendingMove(store: UiStore, targets: ActionTargets): void {
  pending.set(store, targets);
}

/** The targets the move window acts on: those it opened with, else the current ones. */
export function pendingMove(store: UiStore): ActionTargets {
  return pending.get(store) ?? currentTargets(store);
}

/** Moves the move window's targets to `folderId`; `false` when the core refused. */
export async function moveToFolder(ctx: WriteContext, folderId: FolderId): Promise<boolean> {
  const targets = pendingMove(ctx.store);
  pending.delete(ctx.store);
  const result = await runMailAction(ctx, targets, {
    type: "move",
    target: { folder_id: folderId },
  });
  return result !== null;
}
