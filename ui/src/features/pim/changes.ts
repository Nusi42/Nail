/**
 * Moving, resizing and deleting occurrences from the calendar views (drag
 * and drop, keyboard). A change of a recurring event first asks "this /
 * this and following / all" (`pim-edit-scope` dialog, via
 * `PimState.pendingChange`); the write then goes to the core:
 *
 * - `this`: an override of the occurrence (its own fields, new times);
 * - `this-and-following`: the series is split at the occurrence, the new
 *   series starts at the new time (the core returns it as `split`);
 * - `all`: the series' start and end move by the same amount.
 */
import type { StoreApi } from "zustand/vanilla";
import type { EditScope, EventFields, EventInstance, PimTime } from "../../api/generated";
import type { UiStore } from "../../state/uiStore";
import { deleteEvent, getEvent, saveEvent, type PimWriteContext } from "./queries";
import { SCOPE_DIALOG } from "./names";
import type { PimState } from "./store";
import { pimTimeIn, sameKindAs, sameTime, shiftPimTime } from "./time";
import { DAY_MS } from "../../state/clock";

/** Wall-clock difference between two times of the same kind. */
export function wallDelta(from: PimTime, to: PimTime): { days: number; minutes: number } {
  if (from.type === "date" || to.type === "date") {
    if (from.type !== "date" || to.type !== "date") return { days: 0, minutes: 0 };
    const a = Date.parse(`${from.date}T00:00:00Z`);
    const b = Date.parse(`${to.date}T00:00:00Z`);
    return { days: Math.round((b - a) / DAY_MS), minutes: 0 };
  }
  const a = Date.parse(`${from.local}Z`);
  const b = Date.parse(`${to.local}Z`);
  return { days: 0, minutes: Math.round((b - a) / 60_000) };
}

/**
 * New start / end of an occurrence moved by `days` and `minutes` as seen in
 * the viewer's zone (`tz`), written in the occurrence's own kind and zone.
 * `resize` moves only the end (never before start + 15 minutes).
 */
export function movedTimes(
  instance: EventInstance,
  days: number,
  minutes: number,
  tz: string,
  resize = false,
): { start: PimTime; end: PimTime } {
  if (instance.start.type === "date") {
    return resize
      ? { start: instance.start, end: shiftPimTime(instance.end, days, 0) }
      : { start: shiftPimTime(instance.start, days, 0), end: shiftPimTime(instance.end, days, 0) };
  }
  const start = pimTimeIn(instance.start, tz);
  const end = pimTimeIn(instance.end, tz);
  const shift = (value: typeof start) => value.add({ days }).add({ minutes });
  if (resize) {
    let newEnd = shift(end);
    const minimum = start.add({ minutes: 15 });
    if (newEnd.compare(minimum) < 0) newEnd = minimum;
    return { start: instance.start, end: sameKindAs(instance.end, newEnd) };
  }
  return {
    start: sameKindAs(instance.start, shift(start)),
    end: sameKindAs(instance.end, shift(end)),
  };
}

/** Writes a move of an occurrence with a scope. */
export async function applyMove(
  ctx: PimWriteContext,
  instance: EventInstance,
  start: PimTime,
  end: PimTime,
  scope: EditScope,
): Promise<void> {
  const event = await getEvent(ctx, instance.event_id);
  const rid = instance.recurrence_id;
  if (!event.recurrence || rid === null) {
    await saveEvent(
      ctx,
      { mode: "update", eventId: event.id, scope: "all", recurrenceId: null },
      { ...event.fields, start, end },
      event.recurrence,
    );
    return;
  }
  if (scope === "this") {
    const override = event.overrides.find((o) => sameTime(o.recurrence_id, rid));
    const base: EventFields = override?.fields ?? event.fields;
    await saveEvent(
      ctx,
      { mode: "update", eventId: event.id, scope, recurrenceId: rid },
      { ...base, start, end },
      event.recurrence,
    );
    return;
  }
  if (scope === "this-and-following") {
    await saveEvent(
      ctx,
      { mode: "update", eventId: event.id, scope, recurrenceId: rid },
      { ...event.fields, start, end },
      event.recurrence,
    );
    return;
  }
  // All: move the series by what the occurrence moved.
  const startDelta = wallDelta(instance.start, start);
  const endDelta = wallDelta(instance.end, end);
  await saveEvent(
    ctx,
    { mode: "update", eventId: event.id, scope: "all", recurrenceId: rid },
    {
      ...event.fields,
      start: shiftPimTime(event.fields.start, startDelta.days, startDelta.minutes),
      end: shiftPimTime(event.fields.end, endDelta.days, endDelta.minutes),
    },
    event.recurrence,
  );
}

/** Deletes an occurrence (or the event) with a scope. */
export async function applyDelete(
  ctx: PimWriteContext,
  instance: EventInstance,
  scope: EditScope,
): Promise<void> {
  const recurring = instance.recurring && instance.recurrence_id !== null;
  await deleteEvent(
    ctx,
    instance.event_id,
    recurring ? scope : "all",
    recurring ? instance.recurrence_id : null,
  );
}

export interface ChangeContext extends PimWriteContext {
  store: UiStore;
  pim: StoreApi<PimState>;
}

/** Moves an occurrence: at once for single events, after asking for recurring ones. */
export async function requestMove(
  ctx: ChangeContext,
  instance: EventInstance,
  start: PimTime,
  end: PimTime,
): Promise<void> {
  if (instance.read_only) return;
  if (instance.recurring && instance.recurrence_id !== null) {
    ctx.pim.getState().setPendingChange({ kind: "move", instance, start, end });
    ctx.store.getState().openWindow(SCOPE_DIALOG);
    return;
  }
  await applyMove(ctx, instance, start, end, "all");
}

/** Deletes an occurrence: asks the scope for recurring ones. */
export async function requestDelete(ctx: ChangeContext, instance: EventInstance): Promise<void> {
  if (instance.read_only) return;
  if (instance.recurring && instance.recurrence_id !== null) {
    ctx.pim.getState().setPendingChange({ kind: "delete", instance });
    ctx.store.getState().openWindow(SCOPE_DIALOG);
    return;
  }
  await applyDelete(ctx, instance, "all");
}
