/**
 * The plug-in's UI as component trees (JSX through @nail/plugin-api). Every
 * text comes from locales/*.ftl through `nail.l10n`.
 */
import { nail, type DialogSpec, type PluginCollection, type PluginMessage } from "@nail/plugin-api";

import { isPast, senderOf, type FollowUp } from "./reminders";

/** Button ids of the dialog. */
export const CREATE = "create";
export const CANCEL = "cancel";

const t = (id: string, args?: Record<string, string | number>) => nail.l10n.t(id, args);

/** The "Follow up" dialog for a message. */
export function followUpDialog(
  message: PluginMessage,
  suggested: string,
  lists: PluginCollection[],
  existing: FollowUp | null,
  remind: boolean,
): DialogSpec {
  const sender = senderOf(message);
  const defaultList = lists.find((l) => l.default) ?? lists[0];
  return {
    title: t("dialog-title"),
    size: "small",
    body: (
      <stack>
        <text style="strong">
          {t("dialog-about", { subject: message.subject || t("no-subject") })}
        </text>
        {sender && <text style="muted">{t("dialog-from", { sender })}</text>}
        {existing && <text style="muted">{t("dialog-existing", { due: existing.due })}</text>}
        <date id="due" label={t("field-due")} default={suggested} required />
        {lists.length > 1 && (
          <select
            id="list"
            label={t("field-list")}
            options={lists.map((l) => ({ value: l.id, label: l.name }))}
            default={defaultList ? [defaultList.id] : []}
            required
          />
        )}
        <textarea
          id="note"
          label={t("field-note")}
          placeholder={t("field-note-placeholder")}
          default=""
          rows={3}
          maxLength={500}
        />
        <checkbox id="remind" label={t("field-remind")} default={remind} />
      </stack>
    ),
    buttons: [
      { id: CREATE, label: t("button-create"), primary: true },
      { id: CANCEL, label: t("button-cancel") },
    ],
  };
}

/** Button id of the panel. */
export const REMOVE_PAST = "remove-past";

/** The "Follow-ups" panel: every follow-up, soonest first. */
export function duePanel(items: FollowUp[], today: string) {
  if (items.length === 0) {
    return <text style="muted">{t("panel-empty")}</text>;
  }
  const overdue = items.filter((f) => isPast(f, today)).length;
  return (
    <stack>
      {overdue > 0 && <text style="error">{t("panel-overdue", { count: overdue })}</text>}
      <table
        columns={[
          { label: t("column-due") },
          { label: t("column-subject") },
          { label: t("column-note") },
        ]}
        rows={items.map((f) => [f.due, f.subject, f.note])}
      />
      {overdue > 0 && <button id={REMOVE_PAST} label={t("remove-past")} />}
    </stack>
  );
}
