# Nail — English UI strings: composer and outbox.

## Composer

composer-empty = No message is being written.
composer-field-to = { common-header-to }
composer-field-cc = { common-cc }
composer-field-bcc = { action-field-bcc }
composer-field-subject = { common-subject }
composer-no-identity = Choose a sender
composer-add-cc = Add Cc
composer-add-bcc = Add Bcc
composer-send = { common-send }
composer-schedule = Schedule
composer-send-later = Send later…
composer-send-at = { action-field-send-at }
composer-saved = Draft saved.
composer-discard = { common-discard }
composer-error-no-recipients = Add at least one recipient.
composer-error-invalid-address = These addresses are not valid: { $addresses }
composer-error-no-identity = Choose who the message is from.
composer-error-send-at = Choose a valid date and time to send.
# Put above the quoted original in a reply.
composer-quote-header = On { $date }, { $name } wrote:
composer-forward-header = ---------- Forwarded message ----------

## Outbox

outbox-empty = Nothing is waiting to be sent.
outbox-recipients = To: { $recipients }
outbox-send-at = Due { $time }
outbox-send-now = Send now
outbox-draft-continue = Continue writing
outbox-state-scheduled = { portability-phase-queued }
outbox-state-sending = Sending
outbox-state-sent = Sent
outbox-state-failed = { common-failed }
outbox-state-cancelled = { common-cancelled }
# Upload of a large message (Microsoft 365 sends it in pieces).
outbox-upload-progress = { $percent }% uploaded
