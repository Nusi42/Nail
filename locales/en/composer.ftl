# Nail — English UI strings: rich composer (M1b WP21).
#
# Owned by that work package; message ids start with the prefixes listed in
# ui/src/features/composer/messages.ts. The M1a composer strings
# (fields, send, save, errors) stay in compose.ftl.

## Editing modes

composer-mode-label = Format
composer-mode-plain = Plain text
composer-mode-html = HTML
composer-mode-markdown = Markdown
composer-lossy-warning = Switching from { $from } to { $to } loses formatting that { $to } cannot express.
composer-lossy-confirm = Switch anyway
composer-lossy-cancel = Keep editing

## HTML editor

composer-format-label = Formatting
composer-format-bold = Bold
composer-format-bold-short = B
composer-format-italic = Italic
composer-format-italic-short = I
composer-format-underline = Underline
composer-format-underline-short = U
composer-format-strike = Strikethrough
composer-format-strike-short = S
composer-format-heading = Heading
composer-format-heading-short = H
composer-format-bullet-list = Bulleted list
composer-format-bullet-list-short = •
composer-format-ordered-list = Numbered list
composer-format-ordered-list-short = 1.
composer-format-blockquote = Quote
composer-format-blockquote-short = “ ”
composer-format-code-block = Code block
composer-format-code-block-short = { "{ }" }
composer-format-table = Insert table
composer-format-table-short = Table
composer-format-undo = Undo
composer-format-undo-short = Undo
composer-format-redo = Redo
composer-format-redo-short = Redo
composer-format-add-row = Add row
composer-format-add-row-short = + Row
composer-format-add-column = Add column
composer-format-add-column-short = + Column
composer-format-delete-row = Delete row
composer-format-delete-row-short = − Row
composer-format-delete-column = Delete column
composer-format-delete-column-short = − Column
composer-format-delete-table = Delete table
composer-format-delete-table-short = − Table
composer-format-link = Link
composer-format-link-short = Link
composer-format-image = Insert image
composer-format-image-short = Image
composer-format-source = HTML source
composer-format-source-short = Source
composer-link-label = Link
composer-link-url = Address
composer-link-apply = Apply
composer-link-cancel = Cancel
composer-html-source = HTML source

## Markdown editor

composer-preview-toggle = Preview
composer-preview-title = Preview of the message
composer-preview-failed = The preview could not be rendered.
composer-markdown-part = Also send the Markdown source

## Recipients

composer-suggestions-label = Suggested addresses

## Attachments

composer-attach = Attach…
composer-attachments-label = Attachments
composer-attachment-inline = in text
composer-attachment-remove = Remove { $name }
composer-attachment-remove-short = Remove
composer-uploading =
    { $count ->
        [one] Uploading one file…
       *[other] Uploading { $count } files…
    }
composer-error-uploading = Wait until the attachments are uploaded.
# Words that suggest an attachment (comma separated, whole words, any case).
# Translations list the words of their language.
composer-attachment-keywords = attach, attached, attaching, attachment, attachments, enclosed, enclosure
composer-reminder = The message mentions an attachment, but nothing is attached.
composer-reminder-send-anyway = Send anyway
composer-reminder-dismiss = Dismiss

## Drafts

composer-autosave-saving = Saving draft…
composer-autosave-failed = The draft could not be saved.

## Actions (layout labels)

composer-action-set-mode = Change format
composer-action-toggle-preview = Show or hide the preview
composer-action-toggle-source = Show or hide the HTML source
composer-action-attach = Attach a file
