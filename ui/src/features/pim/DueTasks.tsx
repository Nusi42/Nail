/**
 * The "Due this week" panel of the calendar sidebar (S12, ADR 0246): open
 * tasks due in the week shown from today's week (`ListTasksDue`), each
 * with a checkbox (`CompleteTask`), its title and when it is due, and a
 * field that adds a task due today to the first task list. A task can be
 * dragged onto the time grid to block time for it (`setTaskDrag`; the
 * grid creates the event, the task stays).
 *
 * Parts: `due-tasks`, `due-tasks.title`, `due-tasks.items`,
 * `due-tasks.item` (`data-state~="overdue"`), `due-tasks.check`,
 * `due-tasks.name`, `due-tasks.due`, `due-tasks.add`, `due-tasks.empty`.
 */
import { useId, useState, type SubmitEvent } from "react";
import type { CalendarId, LocalizedMessage, Task } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { useNow } from "../../state/clock";
import { Problem, problemOf, useChangeContext } from "./common";
import { emptyTaskFields } from "./defaults";
import { formatPimTime } from "./fmt";
import { setTaskDrag, useTasksDue } from "./prefs";
import { pimWrite, useCalendars } from "./queries";
import { AddTaskForm, dueMs } from "./TaskList";
import { dateString, rangeInstants, todayIn, viewRange } from "./time";

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
  const week = viewRange("week", today, locale);
  const tasks = useTasksDue(rangeInstants(week, tz), lists, tz);
  const calendars = useCalendars();
  const target = (calendars.data ?? []).find(
    (c) =>
      c.components.includes("task") && !c.read_only && (lists === null || lists.includes(c.id)),
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

  const row = (task: Task) => {
    const name = task.fields.title || t("common-no-title");
    const overdue = dueMs(task, tz) < now;
    return (
      <li
        key={task.id}
        data-part="due-tasks.item"
        data-task={task.id}
        data-state={overdue ? "overdue" : undefined}
        draggable={!task.read_only}
        onDragStart={(event) => {
          setTaskDrag(event.dataTransfer, { taskId: task.id, title: name });
        }}
      >
        <input
          type="checkbox"
          data-part="due-tasks.check"
          checked={false}
          disabled={task.read_only}
          aria-label={t("pim-task-complete", { title: name })}
          onChange={() => void run({ type: "CompleteTask", task_id: task.id, completed: true })}
        />
        <span data-part="due-tasks.name">{name}</span>
        {task.fields.due && (
          <span data-part="due-tasks.due">{formatPimTime(format, task.fields.due, tz)}</span>
        )}
      </li>
    );
  };

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
