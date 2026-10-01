/**
 * Starting a new event or task from the views ("new event at 10:00 on
 * Tuesday", "new all-day event", "create event from email"): fills the
 * editor's target and opens it.
 */
import type { CalendarDate } from "@internationalized/date";
import type { CalendarId, EventFields, TaskFields } from "../../api/generated";
import type { ChangeContext } from "./changes";
import { EVENT_DIALOG, TASK_DIALOG } from "./names";
import { dateString, makePimTime } from "./time";

/** Length of a new event, minutes. */
export const NEW_EVENT_MINUTES = 60;

export function openNewEvent(
  ctx: Pick<ChangeContext, "store" | "pim">,
  day: CalendarDate,
  time: { hour: number; minute: number } | null,
  tz: string,
  calendarId: CalendarId | null = null,
  fields: EventFields | null = null,
): void {
  const start =
    fields?.start ??
    (time === null ? { type: "date" as const, date: dateString(day) } : makePimTime(day, time, tz));
  const end =
    fields?.end ??
    (time === null
      ? { type: "date" as const, date: dateString(day.add({ days: 1 })) }
      : (() => {
          const minutes = time.hour * 60 + time.minute + NEW_EVENT_MINUTES;
          const endDay = day.add({ days: Math.floor(minutes / 1440) });
          const rest = minutes % 1440;
          return makePimTime(endDay, { hour: Math.floor(rest / 60), minute: rest % 60 }, tz);
        })());
  ctx.pim.getState().editEvent({ mode: "new", start, end, calendarId, fields });
  ctx.store.getState().openWindow(EVENT_DIALOG);
}

export function openNewTask(
  ctx: Pick<ChangeContext, "store" | "pim">,
  listId: CalendarId | null,
  parentId: string | null = null,
  fields: TaskFields | null = null,
): void {
  ctx.pim.getState().editTask({ mode: "new", listId, parentId, fields });
  ctx.store.getState().openWindow(TASK_DIALOG);
}
