/**
 * Label / category windows (`gmail.apply-labels`, `exchange.apply-categories`)
 * act like Gmail's label menu: on what was the target when they opened —
 * the selected conversation (every message of it), the picked rows, or a
 * card's message (P3, ADR 0169).
 */
import { currentTargets, type ActionTargets } from "../../actions/mailTargets";
import type { MailActionResult } from "../../api/generated";
import type { WriteContext } from "../../state/mailOps";
import type { UiStore } from "../../state/uiStore";
import { runMailAction } from "./run";

const pending = new WeakMap<UiStore, ActionTargets>();

/** Remembers the targets of the label window that opens now (the current ones). */
export function rememberTagTargets(store: UiStore): void {
  pending.set(store, currentTargets(store));
}

/** The targets the label window acts on: those it opened with, else the current ones. */
export function pendingTagTargets(store: UiStore): ActionTargets {
  return pending.get(store) ?? currentTargets(store);
}

/**
 * Adds and removes tags on the label window's targets as one undoable
 * batch; `null` when the core refused (the toast says why).
 */
export async function applyTags(
  ctx: WriteContext,
  add: string[],
  remove: string[],
): Promise<MailActionResult | null> {
  const targets = pendingTagTargets(ctx.store);
  pending.delete(ctx.store);
  return runMailAction(ctx, targets, { type: "tags", add, remove });
}
