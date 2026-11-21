# Nail — English UI strings: values from the selected mail in the rules
# window (R2, ADR 0222).
#
# Message ids start with `rules-from-message-` (the rules feature's prefix,
# ui/src/features/rules/messages.ts).

# Button beside a condition's value: offers the values of the mail the rules
# window was opened from.
rules-from-message-values = Values from this mail

# The same button when the window was not opened from a mail: it offers the
# values of the message selected in the list.
rules-from-message-values-selected = Values from the selected mail

# Group headers of the button's menu: where in the mail the values come from.
rules-from-message-source-from = { common-header-from }
rules-from-message-source-sender = { rules-field-sender }
rules-from-message-source-reply-to = { rules-field-reply-to }
rules-from-message-source-to = { common-header-to }
rules-from-message-source-cc = { common-cc }
rules-from-message-source-subject = { common-subject }
rules-from-message-source-list-id = { rules-cond-list-id }
rules-from-message-source-account = { common-account }
rules-from-message-source-folder = Folder
rules-from-message-source-header = Header names
rules-from-message-source-tag = { action-field-tags }

# The menu's items; $value is the value as the mail has it.
rules-from-message-kind-address = Address: { $value }
rules-from-message-kind-name = Name: { $value }
rules-from-message-kind-domain = Domain: { $value }
rules-from-message-kind-subject = Whole subject: { $value }
rules-from-message-kind-subject-word = Word: { $value }
rules-from-message-kind-mailing-list = List: { $value }
rules-from-message-kind-account = { $value }
rules-from-message-kind-folder = { $value }
rules-from-message-kind-header-name = { $value }
rules-from-message-kind-tag = { $value }

# Message menus: opens a saved rule (submenu of the rules) with this mail's
# values offered beside its fields.
rules-from-message-add-to-rule = Add to rule…
