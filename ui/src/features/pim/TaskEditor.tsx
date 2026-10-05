/**
 * The task editor (dialog `pim-task-editor`): title, task list (moving it
 * moves its subtasks too), parent task, status, percent done, priority,
 * start and due (each optional, with or without a time), description,
 * location, web page, categories, recurrence (anchored on the due date),
 * alarms (relative to the due date), and the subtasks — each can be
 * completed or opened, and new ones added. A task created from an email
 * shows a link back to it.
 */
import type { CalendarDate } from "@internationalized/date";
import { useId, useMemo, useState, type SubmitEvent } from "react";
import type {
  Calendar,
  LocalizedMessage,
  PimTime,
  Task,
  TaskFields,
  TaskStatus,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { Problem, problemOf, useChangeContext } from "./common";
import { TASK_STATUSES } from "./constants";
import { DateTimeInput, type Clock } from "./DateTimeInput";
import { AlarmEditor } from "./EventEditor";
import { emptyTaskFields } from "./defaults";
import { pimWrite, useCalendars, useTask, useTasks } from "./queries";
import { RecurrenceEditor } from "./RecurrenceEditor";
import { choiceOf, recurrenceOf, type RecurrenceChoice } from "./recurrence";
import { usePim, type TaskEditorTarget } from "./store";
import { isDone } from "./TaskList";
import { localZone, makePimTime, splitPimTime, todayIn, utcString } from "./time";
import { nonEmpty } from "../../text/empty";

/** Priority choices (iCalendar 1 = highest … 9 = lowest; 0 / none = undefined). */
export const PRIORITIES = [
  ["none", null],
  ["high", 1],
  ["medium", 5],
  ["low", 9],
] as const;

interface WhenValue {
  day: CalendarDate;
  time: Clock | null;
}

function whenOf(time: PimTime | null): WhenValue | null {
  if (!time) return null;
  const parts = splitPimTime(time);
  return { day: parts.day, time: parts.time };
}

function pimTimeOf(when: WhenValue | null, tz: string): PimTime | null {
  if (!when) return null;
  return makePimTime(when.day, when.time, when.time ? tz : null);
}

export function TaskEditorDialog({ close }: DialogProps) {
  const t = useT();
  const target = usePim((s) => s.taskEditor);
  const task = useTask(target?.mode === "edit" ? target.taskId : null);
  const calendars = useCalendars();
  if (target === null) {
    return (
      <div data-part="pim.editor" data-state="empty">
        <p data-part="pim.editor.empty">{t("pim-editor-nothing")}</p>
        <button type="button" data-part="form.button" onClick={close}>
          {t("pim-close")}
        </button>
      </div>
    );
  }
  if (calendars.isPending || (target.mode === "edit" && task.isPending)) {
    return (
      <div data-part="pim.editor" data-state="loading" aria-busy="true">
        <p data-part="pim.editor.loading">{t("pim-loading")}</p>
      </div>
    );
  }
  if (task.isError || calendars.isError) {
    return (
      <div data-part="pim.editor" data-state="error">
        <Problem message={problemOf(task.error ?? calendars.error)} />
        <button type="button" data-part="form.button" onClick={close}>
          {t("pim-close")}
        </button>
      </div>
    );
  }
  return (
    <TaskEditor
      key={target.mode === "edit" ? target.taskId : `new-${target.parentId ?? ""}`}
      target={target}
      task={task.data ?? null}
      calendars={calendars.data}
      close={close}
    />
  );
}

interface EditorProps {
  target: TaskEditorTarget;
  task: Task | null;
  calendars: readonly Calendar[];
  close: () => void;
}

export function TaskEditor({ target, task, calendars, close }: EditorProps) {
  const t = useT();
  const ctx = useChangeContext();
  const tz = localZone();
  const ids = {
    title: useId(),
    list: useId(),
    parent: useId(),
    status: useId(),
    percent: useId(),
    priority: useId(),
    description: useId(),
    location: useId(),
    url: useId(),
    categories: useId(),
    subtask: useId(),
  };
  const lists = calendars.filter((c) => c.components.includes("task") && !c.read_only);
  // Initialized once per editor (the component is keyed by its target).
  const [initialFields] = useState<TaskFields>(() =>
    structuredClone(
      task?.fields ??
        (target.mode === "new" && target.fields
          ? { ...target.fields, parent_id: target.parentId ?? target.fields.parent_id }
          : { ...emptyTaskFields(), parent_id: target.mode === "new" ? target.parentId : null }),
    ),
  );
  const [fields, setFields] = useState<TaskFields>(initialFields);
  const [listId, setListId] = useState<string>(
    task?.calendar_id ?? (target.mode === "new" ? target.listId : null) ?? lists[0]?.id ?? "",
  );
  const [start, setStart] = useState<WhenValue | null>(whenOf(initialFields.start));
  const [due, setDue] = useState<WhenValue | null>(whenOf(initialFields.due));
  const [recurrence, setRecurrence] = useState<RecurrenceChoice>(() => {
    const anchor = initialFields.due ?? initialFields.start;
    return anchor ? choiceOf(task?.recurrence ?? null, anchor) : { type: "none" };
  });
  const [subtaskTitle, setSubtaskTitle] = useState("");
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  const siblings = useTasks([listId], true);
  const update = (patch: Partial<TaskFields>) => {
    setFields((f) => ({ ...f, ...patch }));
  };
  const readOnly = task?.read_only === true;

  // Parents: tasks of the same list that are not this task or below it.
  const descendants = useMemo(() => {
    const all = siblings.data ?? [];
    const out = new Set<string>();
    const walk = (id: string) => {
      for (const child of all.filter((x) => x.fields.parent_id === id)) {
        if (out.has(child.id)) continue;
        out.add(child.id);
        walk(child.id);
      }
    };
    if (task) walk(task.id);
    return out;
  }, [siblings.data, task]);
  const parents = (siblings.data ?? []).filter((x) => x.id !== task?.id && !descendants.has(x.id));
  const subtasks = task ? (siblings.data ?? []).filter((x) => x.fields.parent_id === task.id) : [];
  const anchor = pimTimeOf(due, tz) ?? pimTimeOf(start, tz);

  const built = (): TaskFields => ({
    ...fields,
    title: fields.title.trim(),
    start: pimTimeOf(start, tz),
    due: pimTimeOf(due, tz),
    description: fields.description?.trim() ? fields.description : null,
    location: nonEmpty(fields.location?.trim()) ?? null,
    url: nonEmpty(fields.url?.trim()) ?? null,
    categories: fields.categories.map((c) => c.trim()).filter(Boolean),
  });

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    const next = built();
    if (
      next.start &&
      next.due &&
      splitPimTime(next.due).day.compare(splitPimTime(next.start).day) < 0
    ) {
      setProblem({ id: "pim-issue-end-before-start" });
      return;
    }
    setBusy(true);
    setProblem(null);
    try {
      const rec = anchor ? recurrenceOf(recurrence, anchor, task?.recurrence ?? null) : null;
      if (task) {
        if (listId !== task.calendar_id) {
          await pimWrite(
            ctx,
            { type: "MoveTask", task_id: task.id, calendar_id: listId },
            "TaskSaved",
          );
          next.parent_id = null;
        }
        await pimWrite(
          ctx,
          { type: "UpdateTask", task_id: task.id, fields: next, recurrence: rec },
          "TaskSaved",
        );
      } else {
        const saved = await pimWrite(
          ctx,
          { type: "CreateTask", calendar_id: listId, fields: next, recurrence: rec },
          "TaskSaved",
        );
        ctx.pim.getState().showTask(saved.task.id);
      }
      ctx.pim.getState().editTask(null);
      close();
    } catch (error) {
      setProblem(problemOf(error));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!task) return;
    setBusy(true);
    setProblem(null);
    try {
      await pimWrite(
        ctx,
        { type: "DeleteTask", task_id: task.id, with_subtasks: true },
        "PimDeleted",
      );
      ctx.pim.getState().editTask(null);
      close();
    } catch (error) {
      setProblem(problemOf(error));
    } finally {
      setBusy(false);
    }
  };

  const addSubtask = async () => {
    if (!task || !subtaskTitle.trim()) return;
    setProblem(null);
    try {
      await pimWrite(
        ctx,
        {
          type: "CreateTask",
          calendar_id: task.calendar_id,
          fields: { ...emptyTaskFields(subtaskTitle.trim()), parent_id: task.id },
          recurrence: null,
        },
        "TaskSaved",
      );
      setSubtaskTitle("");
    } catch (error) {
      setProblem(problemOf(error));
    }
  };

  const priorityValue =
    PRIORITIES.find(([, v]) => v === fields.priority)?.[0] ?? (fields.priority ? "custom" : "none");
  // Every link of a task is a message link (`TaskLink`).
  const message = fields.links.at(0);

  return (
    <form
      data-part="pim.editor"
      data-kind="task"
      onSubmit={(e) => void submit(e)}
      aria-busy={busy}
      noValidate
    >
      {readOnly && <p data-part="pim.editor.read-only">{t("pim-editor-read-only")}</p>}
      <fieldset data-part="pim.editor.main" disabled={readOnly || busy}>
        <div data-part="form.field" data-field="title">
          <label data-part="form.label" htmlFor={ids.title}>
            {t("pim-task-title")}
          </label>
          <input
            id={ids.title}
            data-part="form.input"
            value={fields.title}
            onChange={(e) => {
              update({ title: e.target.value });
            }}
            // eslint-disable-next-line jsx-a11y/no-autofocus -- the editor dialog opens on request; its title field takes the focus (dialog pattern).
            autoFocus
          />
        </div>
        {message && (
          <p data-part="pim.editor.link-message">
            {t("pim-linked-message", { subject: message.subject ?? "" })}
            {message.message_id && (
              <button
                type="button"
                data-part="form.button"
                onClick={() => {
                  ctx.store.getState().selectMessage(message.message_id);
                }}
              >
                {t("pim-linked-message-show")}
              </button>
            )}
          </p>
        )}
        <div data-part="form.field" data-field="list">
          <label data-part="form.label" htmlFor={ids.list}>
            {t("pim-task-list")}
          </label>
          <select
            id={ids.list}
            data-part="form.select"
            value={listId}
            onChange={(e) => {
              setListId(e.target.value);
            }}
          >
            {lists.map((c) => (
              <option key={c.id} value={c.id}>
                {c.settings.name}
              </option>
            ))}
          </select>
        </div>
        <div data-part="form.field" data-field="parent">
          <label data-part="form.label" htmlFor={ids.parent}>
            {t("pim-task-parent")}
          </label>
          <select
            id={ids.parent}
            data-part="form.select"
            value={fields.parent_id ?? ""}
            onChange={(e) => {
              update({ parent_id: e.target.value === "" ? null : e.target.value });
            }}
          >
            <option value="">{t("pim-task-no-parent")}</option>
            {parents.map((p) => (
              <option key={p.id} value={p.id}>
                {p.fields.title || t("pim-task-untitled")}
              </option>
            ))}
          </select>
        </div>
        <div data-part="form.field" data-field="status">
          <label data-part="form.label" htmlFor={ids.status}>
            {t("pim-task-status")}
          </label>
          <select
            id={ids.status}
            data-part="form.select"
            value={fields.status}
            onChange={(e) => {
              const status = e.target.value as TaskStatus;
              update({
                status,
                percent_complete: status === "completed" ? 100 : fields.percent_complete,
                completed_at:
                  status === "completed" ? (fields.completed_at ?? utcString(Date.now())) : null,
              });
            }}
          >
            {TASK_STATUSES.map((s) => (
              <option key={s} value={s}>
                {t(`pim-task-status-${s}`)}
              </option>
            ))}
          </select>
        </div>
        <div data-part="form.field" data-field="percent">
          <label data-part="form.label" htmlFor={ids.percent}>
            {t("pim-task-percent")}
          </label>
          <input
            id={ids.percent}
            data-part="form.input"
            data-width="short"
            type="number"
            inputMode="numeric"
            min={0}
            max={100}
            value={fields.percent_complete ?? ""}
            onChange={(e) => {
              update({
                percent_complete:
                  e.target.value === ""
                    ? null
                    : Math.min(100, Math.max(0, Math.trunc(Number(e.target.value)) || 0)),
              });
            }}
          />
        </div>
        <div data-part="form.field" data-field="priority">
          <label data-part="form.label" htmlFor={ids.priority}>
            {t("pim-task-priority")}
          </label>
          <select
            id={ids.priority}
            data-part="form.select"
            value={priorityValue}
            onChange={(e) => {
              const found = PRIORITIES.find(([k]) => k === e.target.value);
              if (found) update({ priority: found[1] });
            }}
          >
            {PRIORITIES.map(([key]) => (
              <option key={key} value={key}>
                {t(`pim-priority-${key}`)}
              </option>
            ))}
            {priorityValue === "custom" && (
              <option value="custom">
                {t("pim-priority-custom", { value: fields.priority ?? 0 })}
              </option>
            )}
          </select>
        </div>

        <OptionalWhen
          label={t("pim-task-start")}
          field="start"
          value={start}
          onChange={setStart}
          tz={tz}
        />
        <OptionalWhen label={t("pim-task-due")} field="due" value={due} onChange={setDue} tz={tz} />

        {anchor ? (
          <RecurrenceEditor value={recurrence} onChange={setRecurrence} start={anchor} tz={tz} />
        ) : (
          <p data-part="pim.editor.hint">{t("pim-task-recurrence-needs-date")}</p>
        )}
        <AlarmEditor
          value={fields.alarms}
          onChange={(alarms) => {
            update({ alarms });
          }}
          tz={tz}
          related="end"
        />

        <div data-part="form.field" data-field="description">
          <label data-part="form.label" htmlFor={ids.description}>
            {t("pim-task-description")}
          </label>
          <textarea
            id={ids.description}
            data-part="form.textarea"
            value={fields.description ?? ""}
            onChange={(e) => {
              update({ description: e.target.value });
            }}
          />
        </div>
        <div data-part="form.field" data-field="location">
          <label data-part="form.label" htmlFor={ids.location}>
            {t("pim-event-location")}
          </label>
          <input
            id={ids.location}
            data-part="form.input"
            value={fields.location ?? ""}
            onChange={(e) => {
              update({ location: e.target.value });
            }}
          />
        </div>
        <div data-part="form.field" data-field="url">
          <label data-part="form.label" htmlFor={ids.url}>
            {t("pim-event-url")}
          </label>
          <input
            id={ids.url}
            data-part="form.input"
            type="url"
            value={fields.url ?? ""}
            onChange={(e) => {
              update({ url: e.target.value });
            }}
          />
        </div>
        <div data-part="form.field" data-field="categories">
          <label data-part="form.label" htmlFor={ids.categories}>
            {t("pim-categories")}
          </label>
          <input
            id={ids.categories}
            data-part="form.input"
            value={fields.categories.join(", ")}
            onChange={(e) => {
              update({ categories: e.target.value.split(",") });
            }}
          />
        </div>
      </fieldset>

      {task && (
        <fieldset data-part="pim.editor.subtasks" disabled={readOnly}>
          <legend data-part="form.section.title">{t("pim-task-subtasks")}</legend>
          {subtasks.length === 0 && <p data-part="pim.editor.hint">{t("pim-task-no-subtasks")}</p>}
          <ul data-part="pim.editor.subtask-list">
            {subtasks.map((sub) => (
              <li key={sub.id} data-part="pim.editor.subtask" data-task={sub.id}>
                <label data-part="form.check">
                  <input
                    type="checkbox"
                    checked={isDone(sub)}
                    onChange={() =>
                      void pimWrite(
                        ctx,
                        { type: "CompleteTask", task_id: sub.id, completed: !isDone(sub) },
                        "TaskSaved",
                      ).catch((error: unknown) => {
                        setProblem(problemOf(error));
                      })
                    }
                  />
                  <span>{sub.fields.title || t("pim-task-untitled")}</span>
                </label>
                <button
                  type="button"
                  data-part="form.button"
                  aria-label={t("pim-task-open", { title: sub.fields.title })}
                  onClick={() => {
                    ctx.pim.getState().editTask({ mode: "edit", taskId: sub.id });
                  }}
                >
                  {t("pim-edit")}
                </button>
              </li>
            ))}
          </ul>
          <div data-part="pim.editor.row" data-row="subtask-add">
            <label data-part="form.label" htmlFor={ids.subtask}>
              {t("pim-task-subtask-new")}
            </label>
            <input
              id={ids.subtask}
              data-part="form.input"
              value={subtaskTitle}
              onChange={(e) => {
                setSubtaskTitle(e.target.value);
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void addSubtask();
                }
              }}
            />
            <button
              type="button"
              data-part="form.button"
              data-action="add-subtask"
              disabled={!subtaskTitle.trim()}
              onClick={() => void addSubtask()}
            >
              {t("pim-task-add")}
            </button>
          </div>
        </fieldset>
      )}

      <Problem message={problem} />
      <div data-part="pim.editor.actions">
        {!readOnly && (
          <button
            type="submit"
            data-part="form.button"
            data-variant="primary"
            disabled={busy || listId === ""}
          >
            {t("pim-save")}
          </button>
        )}
        {task && !readOnly && (
          <button
            type="button"
            data-part="form.button"
            data-variant="danger"
            disabled={busy}
            onClick={() => void remove()}
          >
            {subtasks.length > 0
              ? t("pim-task-delete-with-subtasks", { count: subtasks.length })
              : t("pim-delete")}
          </button>
        )}
        <button
          type="button"
          data-part="form.button"
          onClick={() => {
            ctx.pim.getState().editTask(null);
            close();
          }}
        >
          {t("pim-cancel")}
        </button>
      </div>
    </form>
  );
}

function OptionalWhen({
  label,
  field,
  value,
  onChange,
  tz,
}: {
  label: string;
  field: string;
  value: WhenValue | null;
  onChange: (value: WhenValue | null) => void;
  tz: string;
}) {
  const t = useT();
  return (
    <fieldset data-part="pim.editor.section" data-section={field}>
      <legend data-part="form.section.title">{label}</legend>
      <label data-part="form.check" data-field={`${field}-set`}>
        <input
          type="checkbox"
          checked={value !== null}
          onChange={(e) => {
            onChange(e.target.checked ? { day: todayIn(tz), time: null } : null);
          }}
        />
        <span>{t("pim-task-has-date", { field: label })}</span>
      </label>
      {value && (
        <>
          <label data-part="form.check" data-field={`${field}-time`}>
            <input
              type="checkbox"
              checked={value.time !== null}
              onChange={(e) => {
                onChange({ ...value, time: e.target.checked ? { hour: 17, minute: 0 } : null });
              }}
            />
            <span>{t("pim-task-with-time")}</span>
          </label>
          <DateTimeInput
            label={label}
            field={field}
            day={value.day}
            time={value.time}
            tz={tz}
            onChange={(day, time) => {
              onChange({ day, time });
            }}
          />
        </>
      )}
    </fieldset>
  );
}
