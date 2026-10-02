# Nail — English UI strings: printing and saving as PDF (M4c WP124;
# contract ADR 0094).
#
# Owned by WP124; message ids start with the prefixes listed in
# ui/src/features/print/messages.ts. The contract's ids (nail_api::print)
# are pre-filled.

print-title = Print
window-print = Print

## Contract ids (AgendaLayout, PageSize, PageOrientation, HeaderDetail, PrintTargetIssue)

print-agenda-layout-day = Day
print-agenda-layout-week = Week
print-agenda-layout-month = Month
print-agenda-layout-list = List
print-page-size-a4 = A4
print-page-size-a5 = A5
print-page-size-letter = Letter
print-page-size-legal = Legal
print-orientation-portrait = Portrait
print-orientation-landscape = Landscape
print-headers-basic = Basic headers
print-headers-full = All headers
print-target-issue-invalid-date = Enter a valid date.
print-target-issue-end-before-start = The end is before the start.
print-target-issue-range-too-long = Print at most one year.
print-job-expired = This print preview has expired. Print again.

## Command palette

print-action-message = Print the message…
print-action-thread = Print the conversation…
print-action-event = Print the event…
print-action-agenda = Print an agenda…
print-action-save-pdf = Save the message as PDF…
print-action-keywords = print paper printer pdf
print-action-agenda-keywords = print calendar week month schedule paper pdf
print-action-save-pdf-keywords = pdf export save print document

## The print dialog

print-nothing-selected = Select a message or an event first, or print an agenda of your calendars.
print-choose-agenda = Print an agenda instead
print-what-message = Message: { $title }
print-what-thread = Conversation: { $title }
print-what-event = Event: { $title }
print-what-agenda = Agenda: { $title }
print-agenda-name = { $start } to { $end }
print-agenda-days = Days
print-agenda-start = From
print-agenda-end = To
print-agenda-layout = Layout
print-agenda-calendars = Calendars
print-agenda-calendars-hint = With no calendar ticked, the calendars shown in the calendar are printed.
print-options = Options
print-page-size = Paper size
print-orientation = Orientation
print-headers = Headers
print-quoted-text = Include quoted text
print-attachment-list = List attachments
print-remote-images = Load remote images
print-remote-images-hint = Only where remote content is allowed for this sender anyway.
print-backgrounds = Print background colours
print-now = Print
print-pdf = Save as PDF…
print-pdf-file = PDF file
print-pdf-pick-title = Save as PDF
print-pdf-default-name = Nail print
print-pdf-save = Save PDF
print-pdf-saved = Saved to { $path } ({ $size }).
print-pdf-browser = Nail cannot write PDF files from the browser. Click Print and choose “Save as PDF” as the printer.
print-filter-pdf = PDF documents
print-close = Close

## The print view

print-view-label = Print preview
print-toolbar = Print preview
print-no-subject = (no subject)
print-no-title = (no title)
print-address = { $name } <{ $email }>
print-header-from = From
print-header-to = To
print-header-cc = Cc
print-header-reply-to = Reply to
print-header-date = Date
print-header-message-id = Message-ID
print-quote-omitted = (Quoted text not printed.)
print-attachments = Attachments
print-attachment = { $name } ({ $size })
print-attachment-unnamed = Unnamed attachment
print-thread-count = { $count ->
    [one] 1 message
   *[other] { $count } messages
}
print-event-when = When
print-event-range = { $start } – { $end }
print-event-occurrence = Occurrence
print-event-location = Where
print-event-organizer = Organizer
print-event-attendees = Attendees
print-attendee = { $name }: { $status }
print-agenda-title = { $start } – { $end }
print-agenda-free = No events.
print-agenda-empty = There are no events in these days.
print-all-day = All day
print-time-range = { $start }–{ $end }
