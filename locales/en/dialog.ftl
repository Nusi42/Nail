# Nail — the shared dialog pattern (R6, ADR 0239): the unsaved-changes
# prompt, the settings save bar and the labels of the shared footer.
# Generic verbs (Save, Cancel, Done, Discard, Keep editing) are common-*.

# The window title of the unsaved-changes prompt.
window-unsaved-changes = { common-unsaved-changes }
# Closing (×, Esc, Cancel) or leaving something with unsaved changes;
# $name is what is being edited (a draft's subject, a rule, a settings section).
dialog-unsaved-question = Save changes to “{ $name }”?
dialog-unsaved-detail = Your changes are lost if you don't save them.
# The prompt's second button: close without saving.
dialog-dont-save = Don't save
# The changes cannot be saved (they have problems).
dialog-unsaved-invalid-question = Discard changes to “{ $name }”?
dialog-unsaved-invalid-detail = The changes have problems and cannot be saved. Keep editing to fix them.
# Several windows with unsaved changes close at once (another view, R8);
# their names are listed below the question.
dialog-unsaved-many-question = Save changes to { $count } items?
dialog-unsaved-many-invalid-question = Discard changes to { $count } items?
# What is being edited has no name yet.
dialog-untitled = Untitled
# The prompt opened for a window that has closed meanwhile.
dialog-unsaved-gone = There are no unsaved changes any more.
# The pinned bar of a settings section with unsaved changes; $name is the section.
dialog-unsaved-bar = Unsaved changes to “{ $name }”
# A question's window was opened without a question (it was answered elsewhere).
dialog-question-gone = There is nothing to confirm any more.
# The settings save bar's status for screen readers, once the bar went.
dialog-settings-saved = Changes saved.
dialog-settings-discarded = Changes discarded.
