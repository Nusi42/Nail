# ADR 0162: Images the message body shows are not attachment cards

Status: accepted · 2026-10-05 · P1 side package (extends ADRs 0140, 0105)

## Context

Since P1 (ADR 0140) the core embeds a message's inline `cid:` images into the reader's HTML (`reader_html`, `data:`
URLs). The reader still listed every MIME part from `MessageBody.attachments` as an attachment card, so a
newsletter's `cid:` logo appeared twice: in the body and as a card — "Unnamed attachment" when the part has no file
name.

`AttachmentInfo.inline` could not be used to hide them. It is a guess from the part's headers
(`Content-Disposition: inline`, or a `Content-ID` without disposition): it is also true for a photo a phone sends
"inline" in a plain-text message, which the body never shows and which must stay a card, and false for an image the
sender marked `attachment` but referenced from the HTML anyway.

## Decision

- **A precise flag from the core.** `AttachmentInfo.shown_in_body: bool` (serde default `false`, in the generated
  TypeScript): the sanitized HTML body refers to the part's `Content-ID` with a `cid:` URL and the part is an image
  the reader embeds. nail-mime computes it where the HTML body is built: the sanitizer records every `cid:`
  reference it keeps (`img src`, `background`, `style` and style-sheet `url(...)`, the `<body>` background) whose
  part is in the message's `InlineImages` (`SanitizedHtml::inline_shown`), so references inside removed content
  (scripts, SVG, frames) or in links (`href` is never a `cid:`) count for nothing. `InlineImages` holds only PNG,
  GIF, JPEG and WebP parts by their bytes, within `MAX_INLINE_IMAGE_BYTES` / `MAX_INLINE_IMAGES_BYTES`.
- **Content-IDs match like mail clients match them.** `cid:` in any case, angle brackets, surrounding blanks and
  letter case are ignored, and a `cid:` URL's percent escapes are decoded (RFC 2392, e.g. `%40` for `@`). Before,
  the scheme had to be lower case and the id had to match exactly.
- **`Content-Location` references are not resolved.** The sanitizer drops relative URLs, so a `multipart/related`
  part referenced by `Content-Location` never shows in the body; it stays a card.
- **Every path that builds `AttachmentInfo` sets it.** `parse_message` (every transport: IMAP, POP3, Gmail,
  Exchange, local imports, decrypted mail) and `attachment_content` / `inline_content_by_cid` (computed only for
  parts with a `Content-ID`). `ParsedMessage::has_attachments` (the list row's paperclip) now means "has an
  attachment the reader lists" (`!shown_in_body`) instead of `!inline`; plug-ins' message bodies and the print view
  leave out the same parts. `inline` stays as it was.
- **The reader hides them behind a disclosure button.** `ui/src/widgets/AttachmentList.tsx` (MessageView renders
  it in place of the card list) lists the parts the body does not show; when there are shown ones, a button
  `message-view.attachments.toggle` ("Show all attachments (N)", N = all attachments; expanded "Hide images shown
  in the message", `aria-expanded`) lists every card, the shown ones marked `data-embedded`, still saveable. When
  the body shows every attachment, only the button is there. The list starts collapsed for every message. The
  wrapper `message-view.attachment-list` and the button are styled in `theme/skin.ts`.

## Consequences

- A newsletter's logo or a photo placed in the text is no longer a card, and the list row no longer shows a
  paperclip for it; a real attachment with `Content-Disposition: inline` that the body does not show is a card and
  gets the paperclip (before it had none). Stored list rows keep their `has_attachments` until the message is
  synced again.
- A part referenced as an image that the reader cannot show (not an image, too large, beyond the per-message total)
  stays a card: a sender cannot hide a program behind an `<img src="cid:…">`.
- MCP's message JSON carries the new field as is; the plug-in SDK has no `AttachmentInfo`.
- Tests: nail-mime `tests/shown_in_body.rs` (referenced image, unreferenced inline attachment, referenced
  `attachment`-disposition image with a percent-encoded reference, references that must not count) plus the
  sanitizer unit test and corpus snapshots; nail-core `m1b.rs` (`GetMessage`, `has_attachments`); Vitest
  `widgets/attachmentList.test.tsx` (hidden by default, toggle, axe); e2e `p1-mail.spec.ts` (`expectEmbeddedOnly`
  on the delivered newsletter and the corpus's lab bench photo).
