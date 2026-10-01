# nail-itip interop fixtures

iMIP mail (RFC 6047) in the shapes the common calendar producers send,
used by `tests/interop.rs` (WP81).

**Everything here is synthesized.** The messages were written by hand
(through `generate.py`) to mimic the MIME structure, header set,
transfer encodings, property order and vendor extensions of real
invitations; they contain no real people, accounts or meetings. Names
are historical figures, addresses use reserved example domains
(`example.org`, `example.net`, `contoso.example`, `evil.example`), and
UIDs, message ids and server names are made up.

| File | Mimics | What it exercises |
| --- | --- | --- |
| `google-request.eml` | Google Calendar invitation (weekly series) | `multipart/mixed` → `multipart/alternative` (base64 text, QP HTML, 7bit `text/calendar; method=REQUEST`) + base64 `invite.ics` attachment; `Sender: calendar-notification@google.com` next to the organizer's `From`; IANA `VTIMEZONE` with `X-LIC-LOCATION`; `X-NUM-GUESTS`, `X-GOOGLE-CONFERENCE`, the Meet footer in a folded `DESCRIPTION`; empty `LOCATION:`; a `VALARM` |
| `google-cancel-occurrence.eml` | Google "Cancelled event" for one occurrence | `CANCEL` with `RECURRENCE-ID;TZID=…`, `SEQUENCE:1`, `STATUS:CANCELLED`, only the cancelled attendee listed |
| `outlook-request.eml` | Exchange / Outlook meeting request | `multipart/alternative` with ISO-8859-1 QP text / HTML and a **base64** `text/calendar` part, no attachment; `W. Europe Standard Time` `VTIMEZONE`; a mixed-case attendee address (`Bob@Example.org`); a room as `CUTYPE=RESOURCE`; `SUMMARY;LANGUAGE=`; Teams link; `X-MICROSOFT-CDO-*`, `X-MICROSOFT-DISALLOW-COUNTER:FALSE`, `X-MICROSOFT-LOCATIONS` JSON; a 112-character `UID` folded across lines |
| `outlook-counter.eml` | Outlook "Propose new time" | `COUNTER` listing **every** attendee (the proposer is the sender), `COMMENT;LANGUAGE=`, `X-MS-OLK-ORIGINALSTART` / `-END` |
| `apple-reply-occurrence.eml` | macOS Calendar declining one occurrence | `text/calendar; method=REPLY` sent as a named **attachment** (`iCal-….ics`, quoted-printable); `EMAIL=` parameter, `REQUEST-STATUS`, `X-APPLE-TRAVEL-ADVISORY-BEHAVIOR`; `RECURRENCE-ID` of an occurrence that has no override yet |
| `thunderbird-reply.eml` | Thunderbird accepting the series | inline `text/calendar; method=REPLY` (8bit) plus `invite.ics`; `X-TZINFO` zone with `RDATE` observances; `COMMENT`; `X-MOZ-SEND-INVITATIONS` |
| `spoofed-request.eml` | a forged invitation | single-part `text/calendar` claiming Ada as organizer, sent from another domain with a higher `SEQUENCE` (the card must warn: `sender-not-organizer`) |

`generate.py` writes the files (CRLF line endings, encoded bodies); it is
not needed at test time. The `.gitattributes` here keeps the `.eml` files
byte for byte.
