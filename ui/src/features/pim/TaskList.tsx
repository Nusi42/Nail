/**
 * The `task-list` widget: tasks of the task lists (calendars holding
 * tasks) as a tree of subtasks, with a checkbox to complete each, due date
 * (overdue ones marked), priority, and a quick "add a task" field. A
 * recurring task completed moves on to its next occurrence (the core does
 * that). Options: `lists` (ids; default every visible list),
 * `show-completed`, `sort` (`due`, `priority`, `title`, `manual`),
 * `quick-add`, `header` (M5b review: the Tasks view's header — the title and
 * "New task" as the primary button). "New task" shows only while there is a
 * list to add to.
 *
 * Keyboard: Up / Down move between tasks, Right / Left expand / collapse
 * subtasks, Space completes, Enter opens the editor, Delete deletes.
 */
import { useId, useRef, useState, type SubmitEvent, type KeyboardEvent } from "react";
import { TaskListChooser } from "../exchange";
import type { Calendar, LocalizedMessage, Task } from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import {
  optionBool,
  optionList,
  optionString,
  type WidgetProps,
} from "../../layout/widgetRegistry";
import { useNow } from "../../state/clock";
import { Icon } from "../../widgets/Icon";
import { Problem, problemOf, useChangeContext } from "./common";
import { formatPimTime } from "./fmt";
import { openNewTask } from "./newEvent";
import { TASK_DIALOG } from "./names";
import { pimWrite, useCalendars, useTasks } from "./queries";
import { usePim } from "./store";
import { emptyTaskFields } from "./defaults";
import { localZone, pimTimeIn } from "./time";

export const TASK_SORTS = ["due", "priority", "title", "manual"] as const;
export type TaskSort = (typeof TASK_SORTS)[number];

/** Whether a task is finished. */
export function isDone(task: Task): boolean {
  return task.fields.status === "completed" || task.fields.status === "cancelled";
}

/** When a task is due (ms), `Infinity` without a due date. */
export function dueMs(task: Task, tz: string): number {
  return task.fields.due
    ? pimTimeIn(task.fields.due, tz).toDate().getTime()
    : Number.POSITIVE_INFINITY;
}

/** Sorts sibling tasks. */
export function sortTasks(tasks: readonly Task[], sort: TaskSort, tz: string): Task[] {
  const priority = (t: Task) =>
    t.fields.priority && t.fields.priority > 0 ? t.fields.priority : 10;
  return [...tasks].sort((a, b) => {
    if (isDone(a) !== isDone(b)) return isDone(a) ? 1 : -1;
    switch (sort) {
      case "priority":
        return (
          priority(a) - priority(b) ||
          dueMs(a, tz) - dueMs(b, tz) ||
          a.fields.title.localeCompare(b.fields.title)
        );
      case "title":
        return a.fields.title.localeCompare(b.fields.title);
      case "manual":
        return (
          (a.fields.order ?? Number.MAX_SAFE_INTEGER) - (b.fields.order ?? Number.MAX_SAFE_INTEGER)
        );
      default:
        return (
          dueMs(a, tz) - dueMs(b, tz) ||
          priority(a) - priority(b) ||
          a.fields.title.localeCompare(b.fields.title)
        );
    }
  });
}

/** Tasks as a tree: roots are tasks whose parent is not in the list. */
export function taskTree(
  tasks: readonly Task[],
  sort: TaskSort,
  tz: string,
): { task: Task; level: number; children: number }[] {
  const byId = new Map(tasks.map((t) => [t.id, t]));
  const childrenOf = (id: string | null) =>
    sortTasks(
      tasks.filter((t) =>
        id === null
          ? !t.fields.parent_id || !byId.has(t.fields.parent_id)
          : t.fields.parent_id === id,
      ),
      sort,
      tz,
    );
  const out: { task: Task; level: number; children: number }[] = [];
  const walk = (id: string | null, level: number, seen: Set<string>) => {
    for (const task of childrenOf(id)) {
      if (seen.has(task.id)) continue;
      seen.add(task.id);
      out.push({
        task,
        level,
        children: tasks.filter((t) => t.fields.parent_id === task.id).length,
      });
      walk(task.id, level + 1, seen);
    }
  };
  walk(null, 1, new Set());
  return out;
}

export function priorityKey(priority: number | null): "high" | "medium" | "low" | "none" {
  if (priority === null || priority === 0) return "none";
  if (priority <= 4) return "high";
  if (priority === 5) return "medium";
  return "low";
}

export function TaskList({ node }: WidgetProps) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useChangeContext();
  const ids = { list: useId(), add: useId(), completed: useId() };
  const tz = localZone();
  const fixed = optionList(node, "lists", []);
  const sortOption = optionString(node, "sort", "due");
  const sort: TaskSort = (TASK_SORTS as readonly string[]).includes(sortOption)
    ? (sortOption as TaskSort)
    : "due";
  const quickAdd = optionBool(node, "quick-add", true);
  const header = optionBool(node, "header", false);
  const [showCompleted, setShowCompleted] = useState(optionBool(node, "show-completed", false));
  const [list, setList] = useState<string>("");
  const [title, setTitle] = useState("");
  const [collapsed, setCollapsed] = useState<string[]>([]);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const calendars = useCalendars();
  const taskLists = (calendars.data ?? []).filter(
    (c) => c.components.includes("task") && (fixed.length === 0 || fixed.includes(c.id)),
  );
  const shown = list ? [list] : fixed.length > 0 ? fixed : null;
  const tasks = useTasks(shown, showCompleted);
  const now = useNow();
  const current = usePim((s) => s.taskId);
  const treeRef = useRef<HTMLUListElement>(null);
  const writableLists = taskLists.filter((c) => !c.read_only);
  const addTarget: Calendar | undefined =
    writableLists.find((c) => c.id === list) ?? writableLists[0];

  const rows = taskTree(tasks.data ?? [], sort, tz);
  const hidden = new Set<string>();
  const visible = rows.filter(({ task }) => {
    const parent = task.fields.parent_id;
    if (parent && (collapsed.includes(parent) || hidden.has(parent))) {
      hidden.add(task.id);
      return false;
    }
    return true;
  });

  const run = async (
    request: Parameters<typeof pimWrite>[1],
    expected: Parameters<typeof pimWrite>[2],
  ) => {
    setProblem(null);
    try {
      return await pimWrite(ctx, request, expected);
    } catch (error) {
      setProblem(problemOf(error));
      return null;
    }
  };

  const add = async (event: SubmitEvent) => {
    event.preventDefault();
    const text = title.trim();
    if (!text || !addTarget) return;
    const saved = await run(
      {
        type: "CreateTask",
        calendar_id: addTarget.id,
        fields: emptyTaskFields(text),
        recurrence: null,
      },
      "TaskSaved",
    );
    if (saved) setTitle("");
  };

  const open = (task: Task) => {
    ctx.pim.getState().showTask(task.id);
    ctx.pim.getState().editTask({ mode: "edit", taskId: task.id });
    ctx.store.getState().openWindow(TASK_DIALOG);
  };

  const focusRow = (id: string | undefined) => {
    if (!id) return;
    ctx.pim.getState().showTask(id);
    treeRef.current
      ?.querySelector<HTMLElement>(`[data-task="${id.replace(/["\\]/g, "")}"]`)
      ?.focus();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLUListElement>) => {
    const target =
      event.target instanceof HTMLElement ? event.target.closest<HTMLElement>("[data-task]") : null;
    if (!target || event.target !== target) return;
    const id = target.dataset.task ?? "";
    const index = visible.findIndex((r) => r.task.id === id);
    const row = visible[index];
    if (!row) return;
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        focusRow(visible[index + 1]?.task.id);
        break;
      case "ArrowUp":
        event.preventDefault();
        focusRow(visible[index - 1]?.task.id);
        break;
      case "Home":
        event.preventDefault();
        focusRow(visible[0]?.task.id);
        break;
      case "End":
        event.preventDefault();
        focusRow(visible[visible.length - 1]?.task.id);
        break;
      case "ArrowRight":
        if (row.children > 0 && collapsed.includes(id)) {
          event.preventDefault();
          setCollapsed((c) => c.filter((x) => x !== id));
        }
        break;
      case "ArrowLeft":
        event.preventDefault();
        if (row.children > 0 && !collapsed.includes(id)) setCollapsed((c) => [...c, id]);
        else if (row.task.fields.parent_id) focusRow(row.task.fields.parent_id);
        break;
      case " ":
        event.preventDefault();
        if (!row.task.read_only)
          void run(
            { type: "CompleteTask", task_id: id, completed: !isDone(row.task) },
            "TaskSaved",
          );
        break;
      case "Enter":
        event.preventDefault();
        open(row.task);
        break;
      case "Delete":
        event.preventDefault();
        if (!row.task.read_only)
          void run({ type: "DeleteTask", task_id: id, with_subtasks: true }, "PimDeleted");
        break;
    }
  };

  const active = visible.find((r) => r.task.id === current)?.task.id ?? visible[0]?.task.id ?? null;

  const newTask = (variant: "primary" | undefined) => (
    <button
      type="button"
      data-part="form.button"
      data-variant={variant}
      data-action="new-task"
      onClick={() => {
        openNewTask(ctx, addTarget?.id ?? null);
      }}
    >
      <Icon name="plus" part="form.button.icon" />
      {t("pim-action-new-task")}
    </button>
  );

  return (
    <section data-part="task-list" data-key-context="tasks" aria-label={t("common-tasks")}>
      {header && (
        <div data-part="pim.view-header">
          <h2 data-part="pim.view-header.title">{t("common-tasks")}</h2>
          <div data-part="pim.view-header.actions">{addTarget && newTask("primary")}</div>
        </div>
      )}
      <div data-part="task-list.toolbar">
        {fixed.length !== 1 && (
          <TaskListChooser lists={taskLists} value={list} onChange={setList} />
        )}
        <label data-part="form.check" data-field="show-completed">
          <input
            type="checkbox"
            checked={showCompleted}
            onChange={(e) => {
              setShowCompleted(e.target.checked);
            }}
          />
          <span>{t("pim-tasks-show-completed")}</span>
        </label>
        {!header && addTarget && newTask(undefined)}
      </div>
      {quickAdd && addTarget && (
        <form data-part="task-list.add" onSubmit={(e) => void add(e)}>
          <label data-part="form.label" htmlFor={ids.add}>
            {t("pim-task-quick-add", { list: addTarget.settings.name })}
          </label>
          <input
            id={ids.add}
            data-part="form.input"
            value={title}
            onChange={(e) => {
              setTitle(e.target.value);
            }}
          />
          <button type="submit" data-part="form.button" disabled={title.trim() === ""}>
            {t("common-add")}
          </button>
        </form>
      )}
      <Problem message={problem} />
      {tasks.isError && <Problem message={problemOf(tasks.error)} />}
      {visible.length === 0 ? (
        <p data-part="task-list.empty">{t("pim-tasks-empty")}</p>
      ) : (
        <ul
          ref={treeRef}
          role="tree"
          data-part="task-list.items"
          aria-label={t("common-tasks")}
          onKeyDown={onKeyDown}
        >
          {visible.map(({ task, level, children }) => {
            const done = isDone(task);
            const due = dueMs(task, tz);
            const overdue = !done && due < now;
            const states = [
              done ? "done" : null,
              overdue ? "overdue" : null,
              task.id === current ? "current" : null,
              task.read_only ? "read-only" : null,
            ].filter(Boolean);
            const name = task.fields.title || t("common-no-title");
            return (
              <li
                key={task.id}
                role="treeitem"
                aria-level={level}
                aria-expanded={children > 0 ? !collapsed.includes(task.id) : undefined}
                aria-selected={task.id === current}
                aria-label={t("pim-task-label", {
                  title: name,
                  status: task.fields.status,
                  due: task.fields.due ? formatPimTime(format, task.fields.due, tz) : "none",
                  priority: priorityKey(task.fields.priority),
                })}
                data-part="task-list.item"
                data-task={task.id}
                data-level={level}
                data-state={states.join(" ") || undefined}
                tabIndex={task.id === active ? 0 : -1}
                onFocus={(e) => {
                  if (e.target === e.currentTarget) ctx.pim.getState().showTask(task.id);
                }}
              >
                <input
                  type="checkbox"
                  data-part="task-list.check"
                  tabIndex={-1}
                  checked={done}
                  disabled={task.read_only}
                  aria-label={t(done ? "pim-task-reopen" : "pim-task-complete", { title: name })}
                  onChange={() =>
                    void run(
                      { type: "CompleteTask", task_id: task.id, completed: !done },
                      "TaskSaved",
                    )
                  }
                />
                {children > 0 && (
                  <button
                    type="button"
                    tabIndex={-1}
                    data-part="task-list.toggle"
                    aria-label={t(
                      collapsed.includes(task.id) ? "pim-task-expand" : "pim-task-collapse",
                      { title: name },
                    )}
                    onClick={() => {
                      setCollapsed((c) =>
                        c.includes(task.id) ? c.filter((x) => x !== task.id) : [...c, task.id],
                      );
                    }}
                  >
                    <Icon
                      name={collapsed.includes(task.id) ? "chevron-right" : "chevron-down"}
                      part="task-list.toggle.icon"
                    />
                  </button>
                )}
                <button
                  type="button"
                  tabIndex={-1}
                  data-part="task-list.title"
                  onClick={() => {
                    open(task);
                  }}
                >
                  {name}
                </button>
                {task.fields.due && (
                  <span data-part="task-list.due" data-state={overdue ? "overdue" : undefined}>
                    {formatPimTime(format, task.fields.due, tz)}
                  </span>
                )}
                {priorityKey(task.fields.priority) !== "none" && (
                  <span
                    data-part="task-list.priority"
                    data-priority={priorityKey(task.fields.priority)}
                  >
                    {t(`pim-priority-${priorityKey(task.fields.priority)}`)}
                  </span>
                )}
                {task.recurrence && (
                  <span data-part="task-list.badge" data-badge="recurring">
                    {t("common-repeats")}
                  </span>
                )}
                {children > 0 && (
                  <span data-part="task-list.subtasks">
                    {t("pim-task-subtask-count", {
                      done: (tasks.data ?? []).filter(
                        (x) => x.fields.parent_id === task.id && isDone(x),
                      ).length,
                      count: children,
                    })}
                  </span>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
