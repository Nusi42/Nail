# Writing data

With the write permissions a plug-in can change mail, drafts, events,
contacts and tasks. Every write goes through Nail's operation queue — the
same undo history, sync and log as your own changes — and may first ask
the user in **Confirmations**.

| API | Permission | Kind of change |
| --- | --- | --- |
| `nail.mail.update(ids, changes)` | `mail.organize` | `organize`: read, flagged, tags, move, archive, snooze |
| `nail.mail.delete(ids, { permanent })` | `mail.organize` | `trash`, or `permanent-delete` — always asks |
| `nail.mail.createDraft(draft)` / `updateDraft(id, changes)` | `mail.send` | `draft` |
| `nail.mail.send(draftId, { sendAt })` | `mail.send` | `send` — asks unless allowed |
| `nail.calendar.createEvent` / `updateEvent` | `calendar.write` (+ `mail.send` to mail attendees) | `calendar`, or `invite` when attendees are mailed |
| `nail.calendar.respond(answer)` | `calendar.write` (+ `mail.send` to tell the organizer) | `invite`, or `calendar` with `send: false` |
| `nail.calendar.deleteEvent` | `calendar.write` | `pim-delete` — always asks |
| `nail.contacts.upsert` / `delete` | `contacts.write` | `contact` / `pim-delete` (always asks) |
| `nail.tasks.create` / `update` / `delete` | `tasks.write` | `task` / `pim-delete` (always asks) |

`ids` may be one message id or a list (at most 500), or an object naming
whole conversations, the way a row of the message list shows them:
`{ messageIds, conversations: [{ threadId, folderId }] }`. Read state and
tags then reach every message of each conversation, including your replies
in Sent; Trash reaches the whole thread in its account (archived messages
too; what is in Trash already stays); moves, archive and snooze reach its messages in `folderId`;
`flagged: true` flags its newest message. Mail writes also need
read access to the messages they name, in the accounts the grant covers.

## Results

Every write resolves with a `PluginWriteResult`:

| `status` | Meaning |
| --- | --- |
| `done` | Queued (undoable). `created` holds the ids of what was created — the draft, event, contact or task (`created[0]`, or `createdId(result)`); `confirmationId` is set when the user approved it. |
| `denied` | The user denied it. Nothing changed. |
| `expired` | Nobody decided within the time limit (5 minutes). Nothing changed. |
| `vetoed` | `nail.mail.send` only: a before-send hook stopped the message (`reasons` names the plug-ins). The draft is kept; the user can send it from Nail. |

A denied or expired write is **not** an error — handle it like any other
answer:

```ts
import { createdId, nail } from "@nail/plugin-api";

const result = await nail.tasks.create({
  title: `Follow up: ${message.subject}`,
  due: "2026-10-12",
  links: [message.uri],          // the task links to the message
});
if (result.status !== "done") {
  await nail.ui.notify({ title: nail.l10n.t("not-created"), level: "info" });
  return;
}
const taskId = createdId(result);
```

Errors still reject: `forbidden` (no permission, or an account the grant
does not cover), `not_found` (a message, draft, list or event that does
not exist or that the plug-in may not see), `invalid_request` (a malformed
call: an empty title, two message links on one task, `archive` together
with `moveToFolderId`), `too_large`, and `rate_limited` when 20 of the
plug-in's writes already wait for the user.

## Confirmations

Whether a write asks first depends on its kind and on the plug-in's
policy, which the user sets in the plug-in manager (**Confirmations**:
*Ask first* / *Without asking*):

| Kinds | Default | Can the user change it? |
| --- | --- | --- |
| `trash`, `permanent-delete`, `pim-delete` | Ask first | No — deleting always asks, and a permanent delete is only ever confirmed by the user in Nail |
| `send`, `invite`, `rule` | Ask first | Yes: *Without asking* (or "From now on, let … do this without asking" when approving) |
| `organize`, `draft`, `calendar`, `contact`, `task` | Without asking | Yes: *Ask first* |

A write that asks shows up in **Confirmations** with the plug-in's name
(and, when the plug-in serves an AI agent's tool call, that agent's name),
a summary ("Follow up wants to change the task “Report”."), the items
concerned and what changes (*Now* / *After*). The plug-in's call waits
until the user chooses **Approve** or **Deny**, or the confirmation
expires. Your code needs nothing special for this: `await` the write as
usual, and remember that it may take minutes. (What users see:
[confirmations](../confirmations.md).)

## Arguments

The argument types (`PluginMessageChanges`, `PluginDraftInput`,
`PluginEventInput`, `PluginInviteAnswer`, `PluginContactInput`,
`PluginTaskInput`, …) are listed in the
[API reference](api-reference.md#writing-data). A few points:

- **Dates**: RFC 3339 times (`2026-10-12T09:00:00+02:00`) or `YYYY-MM-DD`
  for all-day events and due dates.
- **Draft bodies** are Markdown unless `format` says `text` or `html`
  (HTML is sanitized). Attachments are bytes (`data: Uint8Array`), base64,
  or an attachment of another message (`fromMessageId` +
  `fromAttachmentIndex`; copying it needs `mail.read.body`).
- **Tasks** go into the default list unless `list` names one of
  `nail.tasks.lists()`; read-only lists (subscriptions) refuse writes. A
  task can link to one message (`links: [message.uri]`).
- **Fields left out** of an update stay as they are.

## Collections

`nail.calendar.calendars()`, `nail.contacts.addressBooks()` and
`nail.tasks.lists()` list what the plug-in may write to: `{ id, name,
default, readOnly }`. `nail.contacts.find(email)` finds contacts by
address (to avoid duplicates before `upsert`).
