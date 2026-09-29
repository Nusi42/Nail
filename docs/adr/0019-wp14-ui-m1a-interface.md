# ADR 0019 (WP14-UI): M1a user interface — windows, shared form parts, writes, mock core

Status: accepted · 2026-09-29 · M1a WP14-UI (number may be changed by the integrator)

## Context

WP14-UI builds the M1a interface (account setup, account / identity /
virtual-account settings, unified inbox, thread view, message writes, move
picker, search bar, plain-text composer with undo send and send later,
outbox, account status) against the WP10 contract (ADR 0012) before the
core implements it. The design leaves open how secondary windows appear in
the web UI, how form-heavy screens stay themeable without hundreds of part
rules, and how the UI reflects queued writes.

## Decisions

**Secondary windows are layout windows shown as modal dialogs.** The
default layout gains `window "composer"`, `"outbox"`, `"move"`,
`"account-setup"` and `"settings"`. `WindowLayer` renders every open window
(UI state `windows`, a stack) as `data-part="window"` over the main window,
with `data-key-context="window"` and `data-key-modal`: Escape closes it,
main-window (global) bindings do not fire inside it, focus moves in on open
and back on close. Only the topmost window is displayed (`data-state="active"`).
Their content is ordinary layout (a user can rearrange the composer or the
settings tabs). The desktop shell can later open the same windows as real
OS windows; nothing in the widgets assumes a dialog. Structural rules
(positioning, flex) live in the compiled base stylesheet; looks in the
theme.

**Key contexts nest.** A focused element's contexts are all its
`data-key-context` ancestors, innermost first (`composer` → `window`). In
text fields only chords with Ctrl / Alt / Meta and Escape are matched, and
only against the field's own contexts, so `ctrl+enter` sends from the body
while typing never triggers mail actions.

**Shared `form.*` parts.** Settings, setup, virtual-account editor, composer
and outbox render their controls with a small shared vocabulary:
`form.section`, `form.field`, `form.label`, `form.input`, `form.select`,
`form.textarea`, `form.check`, `form.hint`, and `form.button` with
`data-action` (what it does) and `data-variant` (`primary`, `danger`). A
theme targets one button as `form.button[action="send"]` or scoped with an
ancestor (`composer form.button`), the same pattern as
`toolbar.button[action=…]`. Widget-specific parts remain for everything
with its own look (`composer.body`, `thread-view.item`, `search-bar.*`, …).

**Writes are optimistic mirrors of queued operations.** `state/mailOps.ts`
sends `UpdateFlags` / `MoveMessages` / `DeleteMessages`; the core queues and
applies them. The UI patches its caches at once (flags change, moved rows
disappear and the next message is selected), records the returned op ids,
and refetches on `MailChanged` / failed `OpStatus`, so the core's state
always wins. Opening a message marks it read (`message-view mark-read`),
quietly: an older core that answers `unsupported` shows no error.

**Undo send lives in the status bar.** After `SendDraft` the status bar item
`send-status` counts down to `send_at` with an Undo button (`CancelSend`,
then the draft reopens in the composer); it then follows `SendStatus`
events. Send-later times further than two minutes away show as "Scheduled
to send …". The composer's content lives in the UI store, so closing the
window never loses a message; drafts are saved in the core only on
**Save draft** or send (autosave is M1b).

**Graceful degradation.** Requests the running core answers with
`unsupported` degrade silently where that makes sense: account statuses
(no indicators), `CheckQuery` (no live errors), `ListThreads` (the list
falls back to messages), `GetThread` (no thread view).

**Identity rules in the UI (interim).** Choosing an identity for a *new*
message from a virtual account's per-domain rules is done in the composer
(`identityForRecipients`) until the core offers a request for it; replies
pick the identity the original was addressed to. *Superseded by ADR 0021:
the core chooses the identity (`ChooseIdentity`).*

**The mock core implements the whole M1a contract.** `MockTransport` now
simulates virtual accounts (role merging, hand mappings, per-account
subtrees, Message-ID dedupe), queued writes with `OpStatus`, threads, the
query language (`api/mockSearch.ts`, with nail-search's error kinds and
UTF-16 positions), discovery / connection tests / OAuth with deterministic
rules, account and virtual-account CRUD, drafts and an outbox driven by
timers. `data.behaviour` injects failures. It is a development and test
double, not a specification: `nail-core` and `nail-search` are
authoritative.

## Consequences

- The Playwright scenarios of WP14 can address every screen by part names,
  roles and Fluent texts; the integrator writes them against the real core.
- `ListAccounts` returning "All accounts" first makes it the default
  selection (the unified inbox), as ADR 0012 intends.
- CLAUDE.md's note that the mock answers new requests with `unsupported` is
  outdated (integrator to update).
