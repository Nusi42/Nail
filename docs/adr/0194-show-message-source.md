# 0194 — Show a message's source

Status: accepted (2026-10-15)

## Context

Requirement: a message's complete data can be shown as text and copied,
including every transport header, for example to see why an HTML mail
looks the way it does or to forward the headers to a provider. The UI had
no way to reach a message's raw bytes: only the MCP resource
`nail://messages/{id}/raw` (scope `raw-source`) served them.

## Decision

- **API.** `Request::GetMessageSource { message_id }` answers
  `Response::MessageSource` (`nail_api::message_source::MessageSource`):
  - `original`: the stored message as received, as text (bytes that are not
    UTF-8 become U+FFFD, line endings unchanged);
  - `decoded`: `SourceChunk`s — the header fields (encoded words decoded,
    folded fields joined), then each MIME part with its header fields and
    its content without transfer encoding in its charset's characters;
    multipart boundaries kept; a part that is not text (or nested deeper
    than `MAX_SOURCE_NESTING` = 32) is an `Omitted` chunk with type, file
    name and size, which the UI turns into one localized line;
  - each view is cut at `MAX_MESSAGE_SOURCE_BYTES` (4 MiB) at a character
    boundary and flagged; `size` is the whole message.
- **Core.** A body synced headers-only is downloaded first, as for
  `GetMessage` and attachments (`attachments::downloaded_raw`, now shared).
  End-to-end encrypted mail: `original` stays encrypted, `decoded` is built
  from the message decrypted in memory (never stored), as the reader shows
  it; `encrypted` says so. Nothing else is fetched from the network.
- **Remote UI.** Needs the `raw-source` scope (like the MCP resource) and a
  readable message (`check_readable`: granted folder, encrypted mail only
  with `encrypted`).
- **UI.** Feature slot `message-source/`: action `source.message` ("Show
  source", icon `file-code`) in the reader's "More", the card's "…" menu
  and the row and message-view menus, and `Ctrl+U` in the list and reader
  (as in Thunderbird and Gmail). Window `message-source:<n>`, one per
  message (opening it again raises it), sized like the connection log
  viewer (`LOG_WINDOW`): Original / Decoded (the shared view switcher),
  Wrap lines (on by default), Copy all (the shown view), the text in a
  focusable `<pre>` so a part can be selected and copied.

## Consequences

- Event and contact sources (iCalendar, vCard) are not covered yet; they
  can follow the same pattern (`source.event`, `source.contact`).
- No "Save as .eml" from this window; the export (`export.open`) covers
  saving messages as files.
