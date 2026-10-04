/**
 * Microsoft To Do lists in the task widget (ADR 0072: To Do lists are task
 * lists of the account's `exchange:<account>` PIM source with
 * `Calendar.exchange.todo`):
 *
 * - mount point `TaskListChooser`: the task widget's list picker with the
 *   lists grouped — each Exchange account's To Do lists (default "Tasks"
 *   first, "Flagged email" last), other mailboxes' lists, then every other
 *   list — instead of one flat list.
 * - mount point `TaskListBadge`: what a task list is (To Do list, flagged
 *   email, another mailbox's list); nothing for other lists.
 */
import { useId } from "react";
import type { Calendar } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useAccounts } from "../../state/queries";
import { groupTaskLists, type TaskListGroup } from "./model";

export interface TaskListChooserProps {
  /** The task lists to choose from. */
  lists: readonly Calendar[];
  /** The chosen list id, `""` = all lists. */
  value: string;
  onChange: (value: string) => void;
  /** Offer "All lists" (`""`). */
  allowAll?: boolean;
}

/** Mount point `TaskListChooser`. */
export function TaskListChooser({ lists, value, onChange, allowAll = true }: TaskListChooserProps) {
  const t = useT();
  const id = useId();
  const accounts = useAccounts();
  const groups = groupTaskLists(lists);
  const accountName = (key: string | null) =>
    accounts.data?.find((a) => a.id === key)?.name ?? key ?? "";
  const groupLabel = (g: TaskListGroup) =>
    g.kind === "todo"
      ? t("exchange-tasks-group-todo", { account: accountName(g.key) })
      : g.kind === "mailbox"
        ? t("exchange-tasks-group-mailbox", { mailbox: g.key ?? "" })
        : t("exchange-tasks-group-other");
  return (
    <div data-part="form.field" data-field="list">
      <label data-part="form.label" htmlFor={id}>
        {t("exchange-tasks-list")}
      </label>
      <select
        id={id}
        data-part="form.select"
        value={value}
        onChange={(e) => {
          onChange(e.target.value);
        }}
      >
        {allowAll && <option value="">{t("exchange-tasks-all")}</option>}
        {groups.map((g) =>
          // Without Exchange lists there is nothing to group.
          groups.length === 1 && g.kind === "other" ? (
            g.lists.map((c) => (
              <option key={c.id} value={c.id}>
                {c.settings.name}
              </option>
            ))
          ) : (
            <optgroup key={`${g.kind}-${g.key ?? ""}`} label={groupLabel(g)}>
              {g.lists.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.settings.name}
                </option>
              ))}
            </optgroup>
          ),
        )}
      </select>
    </div>
  );
}

/** What kind of Exchange task list a list is, `null` for other lists. */
export function taskListKind(list: Calendar): "todo" | "flagged" | "tasks" | "mailbox" | null {
  const info = list.exchange;
  if (!info || !list.components.includes("task")) return null;
  if (info.mailbox) return "mailbox";
  if (info.role === "flagged-emails") return "flagged";
  return info.todo ? "todo" : "tasks";
}

/** Mount point `TaskListBadge`. */
export function TaskListBadge({ list }: { list: Calendar }) {
  const t = useT();
  const kind = taskListKind(list);
  if (kind === null) return null;
  return (
    <span data-part="exchange.task-list-badge" data-kind={kind}>
      {kind === "mailbox"
        ? t("exchange-tasks-kind-mailbox", { mailbox: list.exchange?.mailbox ?? "" })
        : t(`exchange-tasks-kind-${kind}`)}
    </span>
  );
}
