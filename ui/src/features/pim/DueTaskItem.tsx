/**
 * One open task due in the range the calendar shows (R7 S12, ADR 0246):
 * the all-day row of the time grid and the sidebar's "Due this week" panel
 * use the same item. A checkbox completes the task (`CompleteTask`); the
 * task can be dragged onto the time grid to block time for it
 * (`setTaskDrag`, always the task's own title); and Plan (a button, so the
 * keyboard and touch screens can do it too) creates that event at the
 * grid's chosen slot (`PimState.planSlot`), else today at the next whole
 * hour. The task stays open either way.
 *
 * Parts (each caller names its own): the item (`data-task`,
 * `data-state~="read-only"` / `~="overdue"`), the check, the title, the Plan button
 * (`data-action="plan"`, icon `calendar-plus`); anything else (the due
 * time) comes as children.
 */
import { fromAbsolute } from "@internationalized/date";
import type { ReactNode } from "react";
import { MINUTES_PER_DAY, type Task } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useNow } from "../../state/clock";
import { Icon } from "../../widgets/Icon";
import { problemOf, useChangeContext } from "./common";
import { NEW_EVENT_MINUTES, createEventIn, defaultEventCalendar } from "./newEvent";
import { setTaskDrag } from "./prefs";
import { pimWrite, useCalendars } from "./queries";
import { clickRange, dateString, parseDay, todayIn } from "./time";

/** Minutes per hour (the fallback slot is a whole hour). */
const HOUR = 60;

/**
 * Where Plan puts a task without a chosen slot: today (in `tz`) at the
 * next whole hour after `nowMs`, at the latest the day's last hour.
 */
export function fallbackPlanSlot(nowMs: number, tz: string): { day: string; minute: number } {
  const now = fromAbsolute(nowMs, tz);
  const minute = Math.min((now.hour + 1) * HOUR, MINUTES_PER_DAY - HOUR);
  return { day: dateString(todayIn(tz, nowMs)), minute };
}

/** The part names of one due-task item. */
export interface DueTaskParts {
  item: string;
  check: string;
  title: string;
  plan: string;
}

export interface DueTaskItemProps {
  task: Task;
  parts: DueTaskParts;
  /** The viewer's zone (the slot's day and minute are in it). */
  tz: string;
  /** A geometry rule's id (the all-day row places items by it). */
  geom?: string;
  /** Past its due time (`data-state~="overdue"`). */
  overdue?: boolean;
  children?: ReactNode;
}

export function DueTaskItem({
  task,
  parts,
  tz,
  geom,
  overdue = false,
  children,
}: DueTaskItemProps) {
  const t = useT();
  const ctx = useChangeContext();
  const nowMs = useNow();
  const calendars = useCalendars();
  const name = task.fields.title || t("common-no-title");
  const reportProblem = (error: unknown) => {
    ctx.pim.getState().pushToast({
      kind: "problem",
      id: `task-${task.id}-${String(Date.now())}`,
      message: problemOf(error),
    });
  };
  const calendar = defaultEventCalendar(calendars.data ?? []);

  const plan = () => {
    if (!calendar) return;
    const pim = ctx.pim.getState();
    const slot = pim.planSlot ?? fallbackPlanSlot(nowMs, tz);
    const day = parseDay(slot.day);
    if (!day) return;
    const range = clickRange(slot.minute, NEW_EVENT_MINUTES, 0, MINUTES_PER_DAY);
    createEventIn(ctx, calendar, day, range, tz, task.fields.title).then(() => {
      pim.setAnchor(slot.day);
    }, reportProblem);
  };

  return (
    <li
      data-part={parts.item}
      data-geom={geom}
      data-task={task.id}
      data-state={
        [task.read_only ? "read-only" : null, overdue ? "overdue" : null]
          .filter((state) => state !== null)
          .join(" ") || undefined
      }
      draggable={!task.read_only}
      onDragStart={(event) => {
        setTaskDrag(event.dataTransfer, { taskId: task.id, title: task.fields.title });
      }}
    >
      <input
        type="checkbox"
        data-part={parts.check}
        checked={false}
        disabled={task.read_only}
        aria-label={t("pim-task-complete", { title: name })}
        onChange={() => {
          pimWrite(
            ctx,
            { type: "CompleteTask", task_id: task.id, completed: true },
            "TaskSaved",
          ).catch(reportProblem);
        }}
      />
      <span data-part={parts.title}>{name}</span>
      {children}
      {calendar && (
        <button
          type="button"
          data-part={parts.plan}
          data-action="plan"
          aria-label={t("pim-views-task-plan", { title: name })}
          title={t("pim-views-task-plan", { title: name })}
          onClick={plan}
        >
          <Icon name="calendar-plus" part={`${parts.plan}.icon`} />
        </button>
      )}
    </li>
  );
}
