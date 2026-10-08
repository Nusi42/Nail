# Nail — English UI strings: rich composer (M1b WP21).
#
# Owned by that work package; message ids start with the prefixes listed in
# ui/src/features/composer/messages.ts. The M1a composer strings
# (fields, send, save, errors) stay in compose.ftl.

## Editing modes

composer-mode-plain = Plain text
composer-mode-html = HTML
composer-mode-markdown = Markdown
composer-lossy-warning = Switching from { $from } to { $to } loses formatting that { $to } cannot express.
composer-lossy-confirm = Switch anyway

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
composer-format-table-short = { common-table }
composer-format-undo = { common-undo }
composer-format-undo-short = { common-undo }
composer-format-redo = { common-redo }
composer-format-redo-short = { common-redo }
composer-format-add-row = Add row
composer-format-add-row-short = + Row
composer-format-add-column = { common-add-column }
composer-format-add-column-short = + Column
composer-format-delete-row = Delete row
composer-format-delete-row-short = − Row
composer-format-delete-column = Delete column
composer-format-delete-column-short = − Column
composer-format-delete-table = Delete table
composer-format-delete-table-short = − Table
composer-format-link = { common-link }
composer-format-link-short = { common-link }
composer-format-image = Insert image
composer-format-image-short = Image
composer-format-source = { composer-html-source }
composer-format-source-short = Source
composer-link-url = Address
composer-html-source = HTML source

## Markdown editor

composer-preview-title = Preview of the message
composer-preview-failed = The preview could not be rendered.
composer-markdown-part = Also send the Markdown source

## Recipients

composer-suggestions-label = Suggested addresses

## Attachments

composer-attach = Attach…
composer-attachment-inline = in text
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

## Drafts

composer-autosave-saving = Saving draft…
composer-autosave-failed = The draft could not be saved.

## Actions (layout labels)

composer-action-set-mode = Change format
composer-action-toggle-preview = Show or hide the preview
composer-action-toggle-source = Show or hide the HTML source
composer-action-attach = Attach a file

## Editors loaded on demand (M2c WP54)

composer-editor-loading = Loading the editor…
composer-editor-failed = The editor could not be loaded. Reload Nail and try again.

## Several composers and their confirmations (M5c WP172)

# Titles of the modal confirmations (the composer's own title is its subject).
window-composer-close = Unsaved changes
window-composer-discard = Discard message
# { $subject }: the message's subject, or "New message".
composer-close-question = “{ $subject }” has changes that are not saved yet. Save them as a draft before closing?
composer-close-discard = Close without saving

## Pop-out windows (desktop, M5c): why a message stays in the main window

composer-pop-out-plain = this message window cannot open on its own
composer-pop-out-not-saved = the message could not be saved first
composer-discard-question = Discard “{ $subject }”?
composer-discard-saved = Discard “{ $subject }”? Its saved draft is deleted too.
