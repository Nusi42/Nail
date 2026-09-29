# Nail — English UI strings: composer and outbox.

## Composer

composer-label = New message
composer-empty = No message is being written.
composer-field-from = From
composer-field-to = To
composer-field-cc = Cc
composer-field-bcc = Bcc
composer-field-subject = Subject
composer-field-body = Message
composer-no-identity = Choose a sender
composer-add-cc = Add Cc
composer-add-bcc = Add Bcc
composer-send = Send
composer-schedule = Schedule
composer-send-later = Send later…
composer-send-at = Send at
composer-save = Save draft
composer-saved = Draft saved.
composer-discard = Discard
composer-error-no-recipients = Add at least one recipient.
composer-error-invalid-address = These addresses are not valid: { $addresses }
composer-error-no-identity = Choose who the message is from.
composer-error-send-at = Choose a valid date and time to send.
# Put above the quoted original in a reply.
composer-quote-header = On { $date }, { $name } wrote:
composer-forward-header = ---------- Forwarded message ----------
composer-unknown-sender = Unknown sender

## Outbox

outbox-label = Outbox
outbox-empty = Nothing is waiting to be sent.
outbox-recipients = To: { $recipients }
outbox-send-at = Due { $time }
outbox-edit = Edit
outbox-send-now = Send now
# Saved drafts, listed in the outbox window so they can be continued.
outbox-drafts-label = Drafts
outbox-draft-continue = Continue writing
outbox-draft-discard = Discard
outbox-state-scheduled = Waiting
outbox-state-sending = Sending
outbox-state-sent = Sent
outbox-state-failed = Failed
outbox-state-cancelled = Cancelled
