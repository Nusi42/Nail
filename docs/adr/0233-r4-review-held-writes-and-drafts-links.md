# 0233 — R4 review: held-back server copies, dismissed writes, old draft links

Status: accepted (2026-11-22)

## Context

The reviews of wave R4 (ADRs 0230–0232) found gaps behind the maintainer's
report that drafts were lost after 0.8.8:

- 0.8.8 linked a received message (no `\Draft`) that the user continued in
  a Drafts folder as the draft's server copy. R4 stopped creating such
  links, but links already stored stayed, and removing a draft's server
  copy (on every autosave, on send and on discard) deleted any message of
  that folder with the copy's Message-ID, permanently.
  `EditDraftMessage` also returned the linked draft before it checked the
  flag.
- R4's sync skipped a server message while a move or delete of its
  Message-ID out of the folder waited, but recorded nothing: the folder's
  checkpoint moved past it, so it was never fetched again (a second copy
  with the same Message-ID, or a copy left behind by a failed move).
- `DismissFailedOps` claimed that "the next sync shows the server's
  state", but nothing undid the local move or delete of a failed write:
  the row stayed uidless in the target folder or Trash.
- `mail.move-out-of-drafts` checked the selected message but moved the
  current targets (the context menu's row, the picked rows, the selected
  conversation), which can hold real drafts.

## Decision

- **Draft copies.** Removing a draft's server copy deletes only Nail's own
  copy (its derived id) or a message of that folder flagged `\Draft`.
  `EditDraftMessage` returns a linked draft only for a `\Draft` message.
  Schema v23's step unlinks every draft whose stored copy names messages in
  its folder none of which is flagged `\Draft`
  (`nail_store::compose::unlink_received_server_copies`).
- **Held-back server copies.** A move or delete out of a folder holds the
  message's Message-ID back while it is pending, in flight or failed
  (`Store::held_move_outs`, one query per sync batch instead of one per
  message). The sync leaves such a server message out and lists its UID in
  `sync_refetch` (schema v23, cleared with the skipped list on a
  UIDVALIDITY reset). Each folder sync fetches the listed UIDs no write
  holds back any more, together with the skipped ones
  (`Store::refetch_uids`); a message still held back is listed again.
- **Dismiss undoes the local change.** `DismissFailedOps` puts the row of a
  dismissed move or delete back into its source folder with its UID (when
  the UID is still valid, free, and no other write waits for the row); a
  row that is gone (a permanent delete) gets its source UID listed to fetch
  again. The core announces the changed folders (`MailChanged`) and asks
  the accounts' engines for a sync. Retry stays possible until the user
  dismisses.
- **Move out of Drafts** moves only the selected message (a message
  target), never picked rows or a conversation.
- Smaller single definitions found by the review: `ErrorCode::parse`,
  `OpsAccountSummary::waiting` (sharing the rule of `OpsSummary::waiting`),
  `errorReason` in `ui/src/api/client.ts` (the core's text, else the
  class's; also behind `errorMessageOf`), `listOptionId` in
  `ui/src/widgets/rovingFocus.ts`, `FolderRowLabel` in
  `ui/src/widgets/FolderList.tsx`, `messageTarget` and `runOptionsOf`, and
  `MailConfirm` as the one confirmation body (links, attachments, Delete
  permanently, the Move window's question); after a failed attempt it gives
  the focus back to Cancel.

## Consequences

- A draft linked by 0.8.8 to received mail loses the link on upgrade; its
  next autosave uploads a copy of its own and the received message stays.
- While a move waits or failed, a second copy with the same Message-ID is
  not shown in the source folder; it shows once the move is done or
  dismissed.
- A dismissed failed move shows the message in its old folder again at
  once, where the server still has it.
- Amends ADR 0230 ("Dismiss … the next sync shows the server's state") and
  ADR 0231 (the skip of a held-back copy).
