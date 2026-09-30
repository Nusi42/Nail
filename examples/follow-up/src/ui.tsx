/**
 * The plug-in's UI as component trees (JSX through @nail/plugin-api).
 */
import type { DialogSpec, PluginMessage } from "@nail/plugin-api";

import { isPast, senderOf, type Reminder } from "./reminders";
import { strings } from "./strings";

/** Button ids of the dialog. */
export const SAVE = "save";
export const REMOVE = "remove";
export const CANCEL = "cancel";

/** The "Follow up" dialog for a message (with the existing reminder, if any). */
export function followUpDialog(
  message: PluginMessage,
  suggested: string,
  existing: Reminder | null,
): DialogSpec {
  const sender = senderOf(message);
  return {
    title: strings.dialogTitle,
    size: "small",
    body: (
      <stack>
        <text style="strong">{strings.remindAbout(message.subject)}</text>
        {sender && <text style="muted">{strings.from(sender)}</text>}
        <date id="due" label={strings.due} default={existing?.due ?? suggested} required />
        <textarea
          id="note"
          label={strings.note}
          placeholder={strings.notePlaceholder}
          default={existing?.note ?? ""}
          rows={3}
          maxLength={500}
        />
      </stack>
    ),
    buttons: [
      { id: SAVE, label: strings.save, primary: true },
      ...(existing ? [{ id: REMOVE, label: strings.remove }] : []),
      { id: CANCEL, label: strings.cancel },
    ],
  };
}

/** Button id of the panel. */
export const REMOVE_PAST = "remove-past";

/** The "Follow-ups" panel: every reminder, soonest first. */
export function duePanel(reminders: Reminder[], today: string) {
  if (reminders.length === 0) {
    return <text style="muted">{strings.panelEmpty}</text>;
  }
  const overdue = reminders.filter((r) => isPast(r, today)).length;
  return (
    <stack>
      {overdue > 0 && <text style="error">{strings.panelOverdue(overdue)}</text>}
      <table
        columns={[
          { label: strings.columnDue },
          { label: strings.columnSubject },
          { label: strings.columnNote },
        ]}
        rows={reminders.map((r) => [r.due, r.subject, r.note])}
      />
      {overdue > 0 && <button id={REMOVE_PAST} label={strings.removePast} />}
    </stack>
  );
}
