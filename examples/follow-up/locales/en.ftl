# Texts of the "Follow up" example in English: the manifest's `%id%`
# references and the texts the code shows (`nail.l10n.t`).

## Manifest

plugin-name = Follow up
plugin-description = Creates a task to follow up on a message, linked to the message, and reminds you when it is due.
follow-up-title = Follow up…
follow-up-description = Create a task to follow up on this message.
due-title = Follow-ups
remind-title = Due follow-up reminders
setting-days-title = Default delay (days)
setting-days-description = How many days ahead the dialog suggests.
setting-remind-title = Remind me
setting-remind-description = Show a notification when a follow-up is due.

## Dialog

dialog-title = Follow up
dialog-about = Follow up on “{ $subject }”
dialog-from = From { $sender }
dialog-existing = A follow-up task for this message is due { $due }.
field-due = Due
field-list = Task list
field-note = Note
field-note-placeholder = What needs doing?
field-remind = Remind me when it is due
button-create = Create task
button-cancel = Cancel
no-subject = (no subject)

## The task

task-title = Follow up: { $subject }

## Notifications

created = Follow-up task created, due { $due }
not-created-denied = The follow-up task was not created.
not-created-expired = The follow-up task was not created: nobody confirmed it in time.
no-selection = Select a message to follow up on
not-readable = This message cannot be read by the Follow up plug-in
no-task-access = Follow up may not create tasks. Allow “tasks.write” in the plug-in manager.
no-lists = There is no task list to add the follow-up to.
not-saved = The task was created, but the reminder could not be saved
due-today = Follow up today: { $subject }
overdue = Overdue follow-up ({ $due }): { $subject }

## Panel

panel-empty = No follow-ups. Use “Follow up…” on a message to add one.
panel-overdue = { $count ->
    [one] One follow-up is overdue
   *[other] { $count } follow-ups are overdue
}
column-due = Due
column-subject = Subject
column-note = Note
remove-past = Forget past follow-ups
