/**
 * The "Due this week" panel of the calendar sidebar (S12, ADR 0246): open
 * tasks due in the week shown from today's week (`ListTasksDue`), each
 * with a checkbox (`CompleteTask`), its title and when it is due, and a
 * field that adds a task due today to the first task list. A task can be
 * dragged onto the time grid, or planned with its Plan button, to block
 * time for it (`DueTaskItem`; the event is created, the task stays).
 *
 * Parts: `due-tasks`, `due-tasks.title`, `due-tasks.items`,
 * `due-tasks.item` (`data-state~="overdue"`), `due-tasks.check`,
 * `due-tasks.name`, `due-tasks.due`, `due-tasks.plan`, `due-tasks.add`,
 * `due-tasks.empty`.
 */
import { useId, useState, type SubmitEvent } from "react";
import type { CalendarId, LocalizedMessage, Task } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { useNow } from "../../state/clock";
import { Problem, problemOf, useChangeContext } from "./common";
import { emptyTaskFields } from "./defaults";
import { formatPimTime } from "./fmt";
import { DueTaskItem, type DueTaskParts } from "./DueTaskItem";
import { writableTaskLists } from "./newEvent";
import { useCalendarPreferences, useTasksDue, weekStartOf } from "./prefs";
import { pimWrite, useCalendars } from "./queries";
import { AddTaskForm, dueMs } from "./TaskList";
import { DEFAULT_VIEW_OPTIONS, dateString, rangeInstants, todayIn, viewRange } from "./time";

/** The parts of a task in the panel. */
const DUE_TASK_PARTS: DueTaskParts = {
  item: "due-tasks.item",
  check: "due-tasks.check",
  title: "due-tasks.name",
  plan: "due-tasks.plan",
};

export function DueTasks({
  tz,
  lists,
}: {
  tz: string;
  /** Task lists to show (`null` = every visible one). */
  lists: readonly CalendarId[] | null;
}) {
  const t = useT();
  const format = useDateFormat();
  const locale = useLocale();
  const ctx = useChangeContext();
  const titleId = useId();
  const now = useNow();
  const today = todayIn(tz, now);
  const weekStart = weekStartOf(useCalendarPreferences());
  const week = viewRange("week", today, locale, { ...DEFAULT_VIEW_OPTIONS, weekStart });
  const tasks = useTasksDue(rangeInstants(week, tz), lists, tz);
  const calendars = useCalendars();
  const target = writableTaskLists(calendars.data ?? []).find(
    (c) => lists === null || lists.includes(c.id),
  );
  const [title, setTitle] = useState("");
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);

  const run = async (request: Parameters<typeof pimWrite>[1]) => {
    setProblem(null);
    try {
      await pimWrite(ctx, request, "TaskSaved");
      return true;
    } catch (error) {
      setProblem(problemOf(error));
      return false;
    }
  };

  const add = async (event: SubmitEvent) => {
    event.preventDefault();
    const text = title.trim();
    if (!text || !target) return;
    const saved = await run({
      type: "CreateTask",
      calendar_id: target.id,
      fields: { ...emptyTaskFields(text), due: { type: "date", date: dateString(today) } },
      recurrence: null,
    });
    if (saved) setTitle("");
  };

  const row = (task: Task) => (
    <DueTaskItem
      key={task.id}
      task={task}
      parts={DUE_TASK_PARTS}
      tz={tz}
      overdue={dueMs(task, tz) < now}
    >
      {task.fields.due && (
        <span data-part="due-tasks.due">{formatPimTime(format, task.fields.due, tz)}</span>
      )}
    </DueTaskItem>
  );

  return (
    <section data-part="due-tasks" aria-labelledby={titleId}>
      <h3 data-part="due-tasks.title" id={titleId}>
        {t("pim-views-due-title")}
      </h3>
      {tasks.length === 0 ? (
        <p data-part="due-tasks.empty">{t("pim-views-due-empty")}</p>
      ) : (
        <ul data-part="due-tasks.items" aria-labelledby={titleId}>
          {tasks.map(row)}
        </ul>
      )}
      {target && (
        <AddTaskForm
          part="task-list.add"
          label={t("pim-views-due-add", { list: target.settings.name })}
          value={title}
          onChange={setTitle}
          onSubmit={add}
        />
      )}
      <Problem message={problem} />
    </section>
  );
}
