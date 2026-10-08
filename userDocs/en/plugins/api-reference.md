# Plug-in API reference

The plug-in API (`apiVersion` `"1"`), from the
`@nail/plugin-api` package. Everything below is generated from the SDK's
types, which are generated from Nail's own contract, so it matches what
Nail does.

```ts
import { definePlugin, nail, isPluginApiError } from "@nail/plugin-api";
```

A few rules hold for every call:

- Calls that reach Nail return promises. `nail.settings`, `nail.l10n` and
  `nail.log` are synchronous, and so are the registrations (`ctx.hooks`,
  `ctx.jobs`, …).
- Every call is checked against the plug-in's [permissions](permissions.md);
  without one it rejects with an error whose `code` is `forbidden`.
- JSON is exchanged in camelCase (`messageIds`, `hasAttachments`).
- An error that a command handler, panel or `activate` does not catch
  stops the plug-in. An error in a hook, composer / attachment / link
  action, preview, Markdown block, job or MCP tool does not: that one call
  fails (a hook is skipped) and the error is logged and counted.
- Writes resolve with a `PluginWriteResult`: a write the user denied, or
  did not confirm in time, is not an error ([writing data](writing-data.md)).
- Besides `nail`, the sandbox has `console` (written to the plug-in's log),
  `setTimeout`, `clearTimeout` and `queueMicrotask`. There is no `fetch`
  (use `nail.http.fetch`), no `window`, `document`, `process` or `require`.

<!-- BEGIN GENERATED (npm run docs -w packages/plugin-api) -->

## Plug-in entry point

The default export of a plug-in's bundle, and what `activate` receives.

### `definePlugin`

Declares a plug-in:

```ts
function definePlugin(setup: PluginSetup): DefinedPlugin
```

### `PluginSetup`

What a plug-in author writes.

| Member | Type | Description |
| --- | --- | --- |
| `activate?` | `(ctx: PluginContext) => void \| Promise<void>` | Called once when the plug-in starts, after the handlers below are registered. |
| `deactivate?` | `() => void` | Called when the plug-in stops; synchronous (the host does not wait). |
| `commands?` | `Record<string, PluginCommandHandler>` | Command handlers by name (`contributes.commands`). |
| `panels?` | `Record<string, PanelProvider \| PanelRender>` | Panels by name (`contributes.panels`). |
| `hooks?` | `PluginSetupHooks` | Mail hooks. |
| `composer?` | `PluginSetupComposer` | Composer actions and Markdown blocks. |
| `attachments?` | `PluginSetupAttachments` | Attachment previews and actions. |
| `links?` | `Record<string, PluginActionHandler>` | Link actions by name (`contributes.linkActions`). |
| `jobs?` | `Record<string, PluginJobHandler>` | Background job handlers by id (`contributes.jobs`). |
| `mcpTools?` | `Record<string, PluginMcpToolHandler>` | MCP tool handlers by id (`contributes.mcpTools`). |

### `PluginSetupHooks`

Mail hooks declared with `definePlugin`.

| Member | Type | Description |
| --- | --- | --- |
| `arrival?` | `PluginArrivalHook` | New mail matching the hook's filter (`hooks.arrival`). |
| `beforeSend?` | `PluginBeforeSendHook` | Outgoing mail before it is queued (`hooks.beforeSend`). |
| `open?` | `PluginOpenHook` | A message opened in the reader (`hooks.open`). |
| `onBannerAction?` | `PluginBannerActionHandler` | Presses on the buttons of banners the open hook answered. |

### `PluginSetupComposer`

Composer extensions declared with `definePlugin`.

| Member | Type | Description |
| --- | --- | --- |
| `actions?` | `Record<string, PluginActionHandler>` | Composer actions by name (`contributes.composerActions`). |
| `markdownBlocks?` | `Record<string, PluginMarkdownBlockRenderer>` | Markdown block renderers by name (`contributes.markdownBlocks`). |

### `PluginSetupAttachments`

Attachment extensions declared with `definePlugin`.

| Member | Type | Description |
| --- | --- | --- |
| `previews?` | `Record<string, SdkPreviewRenderer>` | Preview renderers by name (`contributes.attachmentPreviews`). |
| `actions?` | `Record<string, PluginActionHandler>` | Attachment actions by name (`contributes.attachmentActions`). |

### `PluginContext`

The context `activate` receives: the host's, with JSX panels and previews.

| Member | Type | Description |
| --- | --- | --- |
| `plugin` | `PluginIdentity` | The plug-in's id and version. |
| `commands` | `NailCommandsApi` | Command handlers. |
| `panels` | `{ register(name: string, panel: PanelProvider \| PanelRender): void; }` | Panel providers; `render` may return JSX. |
| `settings` | `NailSettingsApi` | The plug-in's settings. |
| `hooks` | `PluginHooksRegistry` | Mail hooks (`hooks.arrival`, `hooks.beforeSend`, `hooks.open`). |
| `composer` | `PluginComposerRegistry` | Composer actions and Markdown blocks (`composer`). |
| `attachments` | `SdkAttachmentsRegistry` | Attachment previews and actions (`attachments`). |
| `links` | `PluginLinksRegistry` | Link actions. |
| `jobs` | `PluginJobsRegistry` | Background jobs (`background`). |
| `mcp` | `PluginMcpRegistry` | MCP tools (`mcp.expose`). |
| `l10n` | `NailL10nApi` | The plug-in's own translations. |

### `PluginCommandHandler`

Runs a command; `ctx` says where it was invoked and what is selected.

```ts
type PluginCommandHandler = (ctx: PluginCommandContext) => void | Promise<void>;
```

### `PluginIdentity`

The plug-in's identity.

| Member | Type | Description |
| --- | --- | --- |
| `id` | `PluginId` | Its manifest's `id`. |
| `version` | `string` | Its manifest's `version`. |

## The `nail` object

Import it with `import { nail } from "@nail/plugin-api"`. It wraps the host's global `nail` and accepts JSX where the host wants component trees.

### `SdkNail`

The SDK's `nail` object (see `NailApi` for the host side).

| Member | Type | Description |
| --- | --- | --- |
| `plugin` | `PluginIdentity` | The plug-in's id and version. |
| `apiVersion` | `string` | The API version of the host. |
| `mail` | `NailMailApi` | Mail: reading, organizing, drafts and sending. |
| `calendar` | `NailCalendarApi` | Calendars: reading (`calendar.read`) and writing (`calendar.write`). |
| `contacts` | `NailContactsApi` | Contacts (`contacts.write`). |
| `tasks` | `NailTasksApi` | Tasks (`tasks.write`). |
| `composer` | `NailComposerApi` | Edits of the open draft (`composer`). |
| `ui` | `SdkUiApi` | Commands, panels, dialogs and notifications. |
| `commands` | `NailCommandsApi` | Command handlers. |
| `storage` | `NailStorageApi` | Key-value storage. |
| `http` | `NailHttpApi` | HTTP to declared hosts. |
| `settings` | `NailSettingsApi` | The plug-in's settings. |
| `l10n` | `NailL10nApi` | The plug-in's own translations. |
| `log` | `NailLogApi` | The plug-in's log. |

### `NailMailApi`

`nail.mail`: mail. Headers need `mail.read`, bodies `mail.read.body`, changes `mail.organize`, drafts and sending `mail.send` — each for the message's (or the sending identity's) account.

| Member | Type | Description |
| --- | --- | --- |
| `get` | `(id: MessageId) => Promise<PluginMessage>` | One message's headers. |
| `getBody` | `(id: MessageId) => Promise<PluginMessageBody>` | One message's body. Always rejects with `forbidden` for end-to-end encrypted mail. |
| `search` | `(query: string, options?: PluginMailSearchOptions) => Promise<PluginMessage[]>` | Messages matching a query of Nail's search language (`from:ada has:attachment`), newest first, in the granted accounts. Message text is searched only where `mail.read.body` is granted: elsewhere free text matches sender, recipients and subject, and a query with `body:` or `filename:` rejects with `forbidden` when no account allows it. End-to-end encrypted mail is left out while Nail indexes its decrypted text. |
| `thread` | `(threadId: ThreadId) => Promise<PluginMessage[]>` | The messages of a thread, oldest first. |
| `update` | `(targets: MessageId \| MessageId[] \| PluginMailTargets, changes: PluginMessageChanges) => Promise<PluginWriteResult>` | Flags, tags, read state, moves, archiving, snoozing (at most 500 messages). Takes message ids or `PluginMailTargets` (whole conversations). Needs `mail.organize` (action kind `organize`). |
| `delete` | `(targets: MessageId \| MessageId[] \| PluginMailTargets, options?: PluginDeleteOptions) => Promise<PluginWriteResult>` | Moves messages to Trash, or deletes them for good. Needs `mail.organize`; always asks the user (`trash` / `permanent-delete`). Takes message ids or `PluginMailTargets` (whole conversations). |
| `createDraft` | `(draft: PluginDraftInput) => Promise<PluginWriteResult>` | Saves a new draft (never sends it); `created[0]` is its id. Needs `mail.send` (`draft`). |
| `updateDraft` | `(draftId: DraftId, changes: PluginDraftChanges) => Promise<PluginWriteResult>` | Changes a saved draft. Needs `mail.send` (`draft`). |
| `send` | `(draftId: DraftId, options?: PluginSendOptions) => Promise<PluginWriteResult>` | Sends a saved draft. Needs `mail.send`; asks the user unless they allow this plug-in to send (`send`). Other plug-ins' before-send hooks run as for the user's own mail. |

### `PluginMailSearchOptions`

Options of `NailMailApi.search`.

| Member | Type | Description |
| --- | --- | --- |
| `limit?` | `number` | At most this many messages (the host clamps it). |

### `NailCalendarApi`

`nail.calendar`: the user's calendars. Reading needs `calendar.read`, writing `calendar.write`.

| Member | Type | Description |
| --- | --- | --- |
| `events` | `(from: string, to: string, options?: PluginCalendarEventsOptions) => Promise<PluginCalendarEvent[]>` | The event occurrences overlapping `[from, to)` (RFC 3339 times, at most 92 days apart) in every calendar the user shows, by start. Recurring events come as one entry per occurrence. |
| `calendars` | `() => Promise<PluginCollection[]>` | The calendars the plug-in may write to. Needs `calendar.write`. |
| `createEvent` | `(event: PluginEventInput) => Promise<PluginWriteResult>` | Creates an event. Needs `calendar.write` (`calendar`, or `invite` with invitations). |
| `updateEvent` | `(eventId: EventId, changes: PluginEventChanges) => Promise<PluginWriteResult>` | Changes an event. Needs `calendar.write`. |
| `deleteEvent` | `(eventId: EventId, options?: PluginEventDeleteOptions) => Promise<PluginWriteResult>` | Deletes an event. Needs `calendar.write`; always asks the user (`pim-delete`). |
| `respond` | `(answer: PluginInviteAnswer) => Promise<PluginWriteResult>` | Answers an invitation. Needs `calendar.write` (and `mail.send` to tell the organizer; `invite`, else `calendar`). |

### `PluginCalendarEventsOptions`

Options of `NailCalendarApi.events`.

| Member | Type | Description |
| --- | --- | --- |
| `limit?` | `number` | At most this many events (the host clamps it to 500). |

### `SdkUiApi`

The SDK's `nail.ui`.

| Member | Type | Description |
| --- | --- | --- |
| `registerCommand` | `(name: string, handler: PluginCommandHandler) => void` | Registers the handler of a command of the manifest. |
| `registerPanel` | `(name: string, panel: PanelProvider \| PanelRender) => void` | Registers the provider (or render function) of a panel of the manifest. |
| `showDialog` | `(spec: DialogSpec, options?: PluginDialogOptions) => Promise<PluginDialogResult>` | Shows a dialog; resolves with the button and values when it closes. Needs `ui.dialogs`. |
| `notify` | `(notification: PluginNotificationSpec \| string) => Promise<void>` | Shows a notification; a string is its title. Needs `notifications`. |
| `updatePanel` | `(name: string, content: UiChild) => Promise<void>` | Replaces the content of an open panel. |

### `DialogSpec`

A dialog spec whose body may be JSX.

```ts
type DialogSpec = Omit<PluginDialogSpec, "body"> & { body: UiChild; };
```

### `PluginDialogOptions`

Options of `NailUiApi.showDialog`.

| Member | Type | Description |
| --- | --- | --- |
| `onAction?` | `PluginUiActionHandler` | Called when a `button` node in the body is clicked (the dialog stays open). |

### `PluginUiAction`

A click on a `button` node, with the current values of the tree's inputs.

| Member | Type | Description |
| --- | --- | --- |
| `componentId` | `string` | The button's `id`. |
| `values` | `Record<string, JsonValue>` | The values of the input nodes by id. |

### `PanelProvider`

A panel provider whose `render` may return JSX.

| Member | Type | Description |
| --- | --- | --- |
| `render` | `PanelRender` | Returns the panel's content for the UI's current context. |
| `onAction?` | `PluginUiActionHandler` | Called for a `button` node in the panel. |

### `PanelRender`

Draws a panel; may return JSX.

```ts
type PanelRender = (ctx: PluginCommandContext) => UiChild | Promise<UiChild>;
```

### `NailCommandsApi`

`nail.commands`: the handlers of the manifest's commands. No permission needed.

| Member | Type | Description |
| --- | --- | --- |
| `register` | `(name: string, handler: PluginCommandHandler) => void` | Registers the handler of a command declared in `contributes.commands`. |

### `NailStorageApi`

`nail.storage`: the plug-in's own key-value storage on this device (`storage`). Values are JSON; keys at most 256 bytes, values 1 MiB. Writing past the quota rejects with `too_large`.

| Member | Type | Description |
| --- | --- | --- |
| `get` | `<T extends JsonValue = JsonValue>(key: string) => Promise<T \| null>` | The value of a key, or `null` when it is not set. |
| `set` | `(key: string, value: JsonValue) => Promise<void>` | Sets a key. |
| `delete` | `(key: string) => Promise<boolean>` | Removes a key; whether it existed. |
| `keys` | `(prefix?: string) => Promise<string[]>` | The keys, sorted; only those starting with `prefix` when given. |
| `clear` | `() => Promise<void>` | Removes every key. |
| `usage` | `() => Promise<PluginStorageUsage>` | How much of the quota is used. |

### `PluginStorageUsage`

Space used by `NailStorageApi`.

| Member | Type | Description |
| --- | --- | --- |
| `keys` | `number` | Stored keys. |
| `usedBytes` | `number` | Bytes used (keys plus JSON values). |
| `quotaBytes` | `number` | The quota in bytes (5 MiB unless the user changed it). |

### `NailHttpApi`

`nail.http`: requests to the hosts of the manifest's `network:<host>` permissions, made by Nail's core on the plug-in's behalf: `https:` on the default port (`http:` and ports only for loopback hosts), no credentials in the URL, redirects only to allowed hosts. Any other host rejects with `forbidden`, a malformed request with `invalid_request`.

| Member | Type | Description |
| --- | --- | --- |
| `fetch` | `(url: string, request?: PluginHttpRequest) => Promise<PluginHttpResponse>` | Fetches `url`. |

### `PluginHttpRequest`

A request of `NailHttpApi.fetch`.

| Member | Type | Description |
| --- | --- | --- |
| `method?` | `PluginHttpMethod` | `GET` unless given. |
| `headers?` | `Record<string, string>` | Request headers. `Host`, `Cookie`, `Content-Length`, hop-by-hop and `Proxy-*` headers are refused. |
| `body?` | `string \| Uint8Array \| ArrayBuffer` | The body: text (serialize JSON yourself) or bytes (`Uint8Array`, `ArrayBuffer`); not for `GET` / `HEAD`. |

### `PluginHttpMethod`

Methods of `NailHttpApi.fetch`.

```ts
type PluginHttpMethod = "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE";
```

### `PluginHttpResponse`

The response of `NailHttpApi.fetch` (the body is read completely).

| Member | Type | Description |
| --- | --- | --- |
| `status` | `number` | The HTTP status. |
| `ok` | `boolean` | Whether the status is 200–299. |
| `headers` | `Record<string, string>` | Response headers, names in lower case. |
| `body` | `string` | The body as text (UTF-8; malformed bytes become U+FFFD). |
| `text` | `() => Promise<string>` | The body as text. |
| `json` | `<T = JsonValue>() => Promise<T>` | The body parsed as JSON. |
| `bytes` | `() => Promise<Uint8Array>` | The body's bytes (images, PDFs, archives). |
| `arrayBuffer` | `() => Promise<ArrayBuffer>` | The body's bytes as an `ArrayBuffer`. |

### `NailSettingsApi`

`nail.settings`: the values of the manifest's settings schema, as the user set them on the plug-in's generated settings page (defaults otherwise). Synchronous; no permission needed.

| Member | Type | Description |
| --- | --- | --- |
| `get` | `<T extends JsonValue = JsonValue>(key: string) => T \| undefined` | One setting's value (`undefined` for a key the schema does not have). |
| `all` | `() => Record<string, JsonValue>` | Every setting's value. |
| `onChange` | `(listener: (values: Record<string, JsonValue>) => void) => void` | Called with every value whenever the user changes a setting. |

### `NailL10nApi`

`nail.l10n`: the plug-in's own Fluent catalogs (`contributes.locales`, the same ones `%id%` manifest texts use). Synchronous; no permission needed.

| Member | Type | Description |
| --- | --- | --- |
| `locale` | `string` | The language Nail shows (`en`, `de`, `pt-BR`). |
| `t` | `(id: string, args?: PluginL10nArgs) => string` | Message `id` in that language, else in English, else the id itself (per message, like Nail's own texts). |
| `has` | `(id: string) => boolean` | Whether a catalog of the language or the English one has message `id`. |

### `PluginL10nArgs`

Arguments of a localized message (`{ $name }` in the catalog).

```ts
type PluginL10nArgs = Record<string, string | number | boolean | null | undefined>;
```

### `NailLogApi`

`nail.log`: lines in the plug-in's log (plug-in manager, `nail plugin dev`). `console.*` writes there too. No permission needed.

| Member | Type | Description |
| --- | --- | --- |
| `debug` | `(...args: unknown[]) => void` | A debug line. |
| `info` | `(...args: unknown[]) => void` | An information line. |
| `warn` | `(...args: unknown[]) => void` | A warning. |
| `error` | `(...args: unknown[]) => void` | An error. |

## Writing data

Writes go through Nail's operation queue (undoable, in the audit log) and follow the plug-in's action policies: some ask the user first. Every write resolves with a `PluginWriteResult`.

### `PluginWriteResult`

How a write ended (the same answer MCP's write tools give, in camelCase). Writes that need the user's confirmation resolve only after the user decided (or after the confirmation expired, 5 minutes by default). A denied or expired write is not an error: nothing changed.

**`status: "done"`**

| Member | Type | Description |
| --- | --- | --- |
| `opIds` | `string[]` | The queued operations. |
| `created` | `string[]` | Ids of what was created (draft, event, contact, task). |
| `confirmationId?` | `string` | The confirmation the user approved, if one was needed. |

**`status: "denied"`**

| Member | Type | Description |
| --- | --- | --- |
| `confirmationId` | `string` | The confirmation. |

**`status: "expired"`**

| Member | Type | Description |
| --- | --- | --- |
| `confirmationId` | `string` | The confirmation. |

**`status: "vetoed"`**

| Member | Type | Description |
| --- | --- | --- |
| `reasons` | `string[]` | The reasons (`<plug-in name>: <reason>`). |

### `PluginActionKind`

The kind of a write, as Nail's confirmations and "always allow" policies name it (`nail_api::agents::ActionKind`). Trash, permanent delete and deleting events / contacts / tasks (`pim-delete`) always ask the user; sending, invitations and rule changes ask unless the user allows them for the plug-in; the other kinds run without asking unless the user makes them ask.

```ts
type PluginActionKind = ActionKind;
```

### `ActionKind`

What kind of write an action is.

```ts
type ActionKind = "organize" | "trash" | "permanent-delete" | "draft" | "send" | "calendar" | "invite" | "contact" | "task" | "pim-delete" | "rule";
```

### `isDone`

Whether a write was done (not denied, expired or vetoed).

```ts
function isDone( result: PluginWriteResult, ): result is Extract<PluginWriteResult,
```

### `createdId`

The first id a done write created (the new task, draft, event, contact).

```ts
function createdId(result: PluginWriteResult): string | undefined
```

### `PluginMessageChanges`

Changes of `NailMailApi.update`; fields left out stay as they are.

| Member | Type | Description |
| --- | --- | --- |
| `read?` | `boolean` | Mark read (`true`) or unread (`false`). |
| `flagged?` | `boolean` | Flag or unflag. |
| `addTags?` | `string[]` | Tags to add. |
| `removeTags?` | `string[]` | Tags to remove. |
| `moveToFolderId?` | `FolderId` | Move to this folder of the same account. |
| `archive?` | `boolean` | Move to the account's Archive folder. |
| `snoozeUntil?` | `string` | Snooze until this time (RFC 3339). |

### `PluginDeleteOptions`

Options of `NailMailApi.delete`.

| Member | Type | Description |
| --- | --- | --- |
| `permanent?` | `boolean` | Delete for good instead of moving to Trash. The user always confirms deletions; permanent ones are never confirmed automatically. |

### `PluginDraftInput`

A new draft (`NailMailApi.createDraft`).

| Member | Type | Description |
| --- | --- | --- |
| `identityId?` | `IdentityId` | The sending identity (default: the account's default identity). |
| `to?` | `string[]` | Recipients (`Name <address>` or `address`). |
| `cc?` | `string[]` | Cc recipients. |
| `bcc?` | `string[]` | Bcc recipients. |
| `subject?` | `string` | Subject (default when replying: `Re: <subject>`). |
| `body?` | `string` | The body. |
| `format?` | `PluginTextFormat` | Format of `body` (default `markdown`). |
| `replyToMessageId?` | `MessageId` | Reply to this message (threading headers, quoted original). |
| `replyAll?` | `boolean` | With `replyToMessageId`: reply to all. |
| `forwardMessageId?` | `MessageId` | Forward this message inline. |
| `attachments?` | `PluginDraftAttachment[]` | Attachments (at most 20 per call). |

### `PluginDraftAttachment`

An attachment of a draft: its bytes (`data`, or `dataBase64`), or an attachment of an existing message (copying it needs `mail.read.body`).

| Member | Type | Description |
| --- | --- | --- |
| `filename` | `string` | File name. |
| `mimeType?` | `string` | MIME type (default: from the file name). |
| `data?` | `Uint8Array \| ArrayBuffer` | The content (at most `MAX_ATTACHMENT_BYTES`). |
| `dataBase64?` | `string` | Or the content as base64. |
| `fromMessageId?` | `MessageId` | Or: copy an attachment of this message … |
| `fromAttachmentIndex?` | `number` | … with this index. |

### `PluginDraftChanges`

Changes of `NailMailApi.updateDraft`; fields left out stay as they are.

| Member | Type | Description |
| --- | --- | --- |
| `to?` | `string[]` | New recipients. |
| `cc?` | `string[]` | New Cc recipients. |
| `bcc?` | `string[]` | New Bcc recipients. |
| `subject?` | `string` | New subject. |
| `body?` | `string` | New body. |
| `format?` | `PluginTextFormat` | Format of `body` (default: the draft's). |
| `addAttachments?` | `PluginDraftAttachment[]` | Attachments to add. |
| `removeAttachments?` | `number[]` | Positions of attachments to remove. |

### `PluginSendOptions`

Options of `NailMailApi.send`.

| Member | Type | Description |
| --- | --- | --- |
| `sendAt?` | `string` | Send later (RFC 3339); default: now (after the user's undo-send delay). |

### `PluginTextFormat`

The format of a body or inserted text.

```ts
type PluginTextFormat = ComposerTextFormat;
```

### `PluginCollection`

A calendar or task list a plug-in may write to.

| Member | Type | Description |
| --- | --- | --- |
| `id` | `CalendarId` | Its id. |
| `name` | `string` | Its name. |
| `default` | `boolean` | Whether it is the default one of its kind (for new items). |
| `readOnly` | `boolean` | Read-only (a subscription): writes to it fail. |

### `PluginEventInput`

A new event (`NailCalendarApi.createEvent`).

| Member | Type | Description |
| --- | --- | --- |
| `calendarId?` | `CalendarId` | The calendar (default: the default calendar). |
| `title` | `string` | Title. |
| `start` | `string` | Start: RFC 3339, or `YYYY-MM-DD` for all-day events. |
| `end` | `string` | End (exclusive): RFC 3339, or `YYYY-MM-DD` for all-day events. |
| `tz?` | `string` | IANA zone of the times (default: the system's). |
| `location?` | `string` | Location. |
| `description?` | `string` | Description (plain text). |
| `attendees?` | `string[]` | Attendee addresses. |
| `recurrence?` | `string` | An iCalendar `RRULE` value (`FREQ=WEEKLY;BYDAY=MO`). |
| `sendInvitations?` | `boolean` | Mail invitations to the attendees (also needs `mail.send`; action kind `invite`). |

### `PluginEventChanges`

Changes of `NailCalendarApi.updateEvent`; fields left out stay.

| Member | Type | Description |
| --- | --- | --- |
| `occurrence?` | `string` | For recurring events: the occurrence's original start (RFC 3339). |
| `scope?` | `PluginEditScope` | Which occurrences (default `all`). |
| `title?` | `string` | New title. |
| `start?` | `string` | New start. |
| `end?` | `string` | New end. |
| `tz?` | `string` | New zone. |
| `location?` | `string` | New location. |
| `description?` | `string` | New description. |
| `addAttendees?` | `string[]` | Attendees to add. |
| `removeAttendees?` | `string[]` | Attendees to remove. |
| `sendUpdates?` | `boolean` | Mail the change to the attendees (also needs `mail.send`). |

### `PluginEventDeleteOptions`

Options of `NailCalendarApi.deleteEvent`.

| Member | Type | Description |
| --- | --- | --- |
| `occurrence?` | `string` | For recurring events: the occurrence's original start. |
| `scope?` | `PluginEditScope` | Which occurrences (default `all`). |

### `PluginEditScope`

Which occurrences of a recurring event a change applies to.

```ts
type PluginEditScope = "this" | "this-and-following" | "thisAndFollowing" | "all";
```

### `PluginInviteAnswer`

An answer to an invitation (`NailCalendarApi.respond`).

| Member | Type | Description |
| --- | --- | --- |
| `messageId?` | `MessageId` | The invitation message … |
| `eventId?` | `EventId` | … or the event (exactly one of the two). |
| `occurrence?` | `string` | For recurring events: the occurrence (original start); default: the whole series. |
| `response` | `"accept" \| "tentative" \| "decline"` | The answer. |
| `comment?` | `string` | A note to the organizer. |
| `send?` | `boolean` | Tell the organizer (default `true`; needs `mail.send`, action kind `invite`). |

### `NailContactsApi`

`nail.contacts`: contact writes. Needs `contacts.write`.

| Member | Type | Description |
| --- | --- | --- |
| `addressBooks` | `() => Promise<PluginCollection[]>` | The address books the plug-in may write to. |
| `find` | `(email: string, options?: PluginContactFindOptions) => Promise<PluginContact[]>` | The contacts with this email address (case-insensitive). |
| `upsert` | `(contact: PluginContactInput) => Promise<PluginWriteResult>` | Creates or changes a contact (`contact`). |
| `delete` | `(contactId: string) => Promise<PluginWriteResult>` | Deletes a contact; always asks the user (`pim-delete`). |

### `PluginContact`

A contact as `NailContactsApi.find` returns it.

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | Its id. |
| `addressBookId` | `string` | Its address book. |
| `fullName?` | `string` | Full name. |
| `emails` | `string[]` | Email addresses. |
| `organization?` | `string` | Organization. |

### `PluginContactInput`

A new contact or changes of one (`NailContactsApi.upsert`).

| Member | Type | Description |
| --- | --- | --- |
| `contactId?` | `string` | The contact to change; absent = create one. |
| `addressBookId?` | `string` | For new contacts: the address book (default: the default one). |
| `fullName?` | `string` | Full name. |
| `emails?` | `string[]` | Email addresses (replace the existing ones). |
| `phones?` | `string[]` | Phone numbers (replace the existing ones). |
| `organization?` | `string` | Organization. |
| `title?` | `string` | Job title. |
| `notes?` | `string` | Notes. |

### `PluginContactFindOptions`

Options of `NailContactsApi.find`.

| Member | Type | Description |
| --- | --- | --- |
| `limit?` | `number` | At most this many contacts (the host clamps it). |

### `NailTasksApi`

`nail.tasks`: task writes. Needs `tasks.write`.

| Member | Type | Description |
| --- | --- | --- |
| `lists` | `() => Promise<PluginCollection[]>` | The task lists the plug-in may write to. |
| `create` | `(task: PluginTaskInput) => Promise<PluginWriteResult>` | Creates a task (`task`); `created[0]` is its id. |
| `update` | `(taskId: string, changes: PluginTaskChanges) => Promise<PluginWriteResult>` | Changes a task (`task`). |
| `delete` | `(taskId: string) => Promise<PluginWriteResult>` | Deletes a task; always asks the user (`pim-delete`). |

### `PluginTaskInput`

A new task (`NailTasksApi.create`).

| Member | Type | Description |
| --- | --- | --- |
| `list?` | `CalendarId` | The task list (default: the default one). |
| `listId?` | `CalendarId` | The same as `list` (pass one of them). |
| `title` | `string` | Title. |
| `due?` | `string` | Due: RFC 3339 or `YYYY-MM-DD`. |
| `notes?` | `string` | Notes (plain text). |
| `priority?` | `number` | Priority 1 (highest) … 9 (lowest); 0 = none. |
| `completed?` | `boolean` | Created as done. |
| `links?` | `string[]` | A link to the message the task is about (`nail://messages/<id>`, a message's `uri`; at most one message). Needs `mail.read` for it. |
| `relatedMessageId?` | `MessageId` | The same as a `links` entry, as a message id. |

### `PluginTaskChanges`

Changes of `NailTasksApi.update`; fields left out stay as they are.

| Member | Type | Description |
| --- | --- | --- |
| `title?` | `string` | New title. |
| `due?` | `string` | New due date; `""` removes it. |
| `notes?` | `string` | New notes. |
| `priority?` | `number` | New priority. |
| `completed?` | `boolean` | Done or open. |

## Mail hooks

Declared in `contributes.hooks` and registered with `definePlugin({ hooks })` or `ctx.hooks.register`.

### `PluginHooksRegistry`

Hook registrations (`ctx.hooks`).

| Member | Type | Description |
| --- | --- | --- |
| `register` | `<K extends PluginHookKind>(hook: K, handler: PluginHookHandlers[K]) => void` | Registers the handler of a hook of the manifest (`contributes.hooks`). |
| `onBannerAction` | `(handler: PluginBannerActionHandler) => void` | Registers the handler of banner button presses. |

### `PluginHookMessage`

A message as an arrival or open hook gets it: the headers, plus the body when the plug-in has `mail.read.body` for the account (never for end-to-end encrypted mail).

| Member | Type | Description |
| --- | --- | --- |
| `message` | `PluginMessage` | The message's headers. |
| `body?` | `PluginMessageBody` | Its body, with `mail.read.body`. |

### `PluginArrivalHook`

The arrival hook (`hooks.arrival`): called for each new message that matches the hook's `filter`, after the user's own rules. Answers actions (they need `mail.organize`), or nothing. Budget: 2 s by default.

```ts
type PluginArrivalHook = ( mail: PluginHookMessage, ) => | PluginArrivalResult | null | undefined | void | Promise<PluginArrivalResult | null | undefined | void>;
```

### `PluginArrivalResult`

What an arrival hook answers (`undefined` / `null` = nothing).

| Member | Type | Description |
| --- | --- | --- |
| `actions?` | `Array<PluginMailAction>` | Actions, applied in order (at most `MAX_ARRIVAL_ACTIONS`). |

### `PluginMailAction`

An action an arrival hook asks for (camelCase, tagged by `type`).

**`type: "addTag"`**

| Member | Type | Description |
| --- | --- | --- |
| `tag` | `string` | The tag. |

**`type: "removeTag"`**

| Member | Type | Description |
| --- | --- | --- |
| `tag` | `string` | The tag. |

**`type: "setFlagged"`**

| Member | Type | Description |
| --- | --- | --- |
| `flagged` | `boolean` | Flagged. |

**`type: "setRead"`**

| Member | Type | Description |
| --- | --- | --- |
| `read` | `boolean` | Read. |

**`type: "move"`**

| Member | Type | Description |
| --- | --- | --- |
| `folderId` | `FolderId` | The folder. |

**`type: "archive"`**

### `PluginBeforeSendHook`

The before-send hook (`hooks.beforeSend`): called with every outgoing message before it is queued. Answers headers / content to add or a veto, or nothing. Budget: 3 s by default. A hook that throws or runs out of time is skipped: it never blocks sending.

```ts
type PluginBeforeSendHook = ( message: PluginOutgoingMessage, ) => | PluginBeforeSendResult | null | undefined | void | Promise<PluginBeforeSendResult | null | undefined | void>;
```

### `PluginOutgoingMessage`

A message on its way out, as a before-send hook sees it.

| Member | Type | Description |
| --- | --- | --- |
| `draftId` | `DraftId` | The draft being sent. |
| `accountId` | `AccountId` | The sending account. |
| `identityId` | `IdentityId` | The identity. |
| `from` | `Address` | The sender. |
| `to` | `Array<Address>` | Recipients. |
| `cc` | `Array<Address>` | Cc. |
| `bcc` | `Array<Address>` | Bcc. |
| `subject` | `string` | Subject. |
| `text` | `string` | The body as plain text (the text rendering of HTML / Markdown); empty when the message will be end-to-end encrypted (hooks never see encrypted content). |
| `html?` | `string` | The HTML body as it will be sent, if any (never for a message that will be end-to-end encrypted). |
| `attachments` | `Array<PluginAttachment>` | Attachments (names, types, sizes). |
| `inReplyTo?` | `MessageId` | The message being replied to or forwarded. |
| `encrypted` | `boolean` | Whether it will be signed / encrypted (hooks cannot change that; content added to an encrypted message is encrypted with it). |

### `PluginBeforeSendResult`

What a before-send hook answers (`undefined` / `null` = nothing).

| Member | Type | Description |
| --- | --- | --- |
| `veto?` | `PluginVetoSpec` | Stop sending (the user can override it). |
| `headers?` | `Array<PluginHeader>` | Headers to add (at most `MAX_ADDED_HEADERS`). |
| `addContent?` | `Array<PluginContentAddition>` | Content to add (at most `MAX_ADDED_CONTENT` bytes in all). |

### `PluginVetoSpec`

A before-send veto as the plug-in answers it.

| Member | Type | Description |
| --- | --- | --- |
| `reason` | `string` | Why (shown to the user with the plug-in's name; at most `MAX_HOOK_TEXT` characters). |

### `PluginHeader`

A header a before-send hook adds.

| Member | Type | Description |
| --- | --- | --- |
| `name` | `string` | The name (`is_plugin_header_name`: `X-…`). |
| `value` | `string` | The value (one line). |

### `PluginContentAddition`

Content a before-send hook adds.

| Member | Type | Description |
| --- | --- | --- |
| `position?` | `PluginContentPosition` | Where. |
| `text` | `string` | The text (for the plain-text part, and the HTML part when `html` is absent). |
| `html?` | `string` | HTML for the HTML part (sanitized like a message body). |

### `PluginContentPosition`

Where added content goes.

```ts
type PluginContentPosition = "top" | "bottom";
```

### `veto`

A before-send answer that stops sending with `reason` (the user can "Send anyway").

```ts
function veto(reason: string): PluginBeforeSendResult
```

### `addContent`

A before-send answer that adds content at the top or bottom.

```ts
function addContent( text: string, options:
```

### `mergeBeforeSend`

Merges before-send answers (vetoes: the first wins).

```ts
function mergeBeforeSend( ...results: (PluginBeforeSendResult | null | undefined)[] ): PluginBeforeSendResult
```

### `PluginOpenHook`

The open hook (`hooks.open`): called when the user opens a message that matches the hook's `filter`. Answers banners and badges, or nothing. Budget: 1 s by default.

```ts
type PluginOpenHook = ( mail: PluginHookMessage, ) => PluginOpenResult | null | undefined | void | Promise<PluginOpenResult | null | undefined | void>;
```

### `PluginOpenResult`

What an open hook answers (`undefined` / `null` = nothing).

| Member | Type | Description |
| --- | --- | --- |
| `banners?` | `Array<PluginBannerSpec>` | Banners (at most `MAX_OPEN_ITEMS`). |
| `badges?` | `Array<PluginBadgeSpec>` | Badges (at most `MAX_OPEN_ITEMS`). |

### `PluginBannerSpec`

A banner an open hook shows above a message.

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | Its id (reported with button presses; a contribution name). |
| `level?` | `PluginNotificationLevel` | Its level (styling). |
| `text` | `string` | The text (at most `MAX_HOOK_TEXT` characters). |
| `buttons?` | `Array<PluginBannerButton>` | Buttons; pressing one calls the plug-in's `onBannerAction` (`Banner`). |

### `PluginBannerButton`

A banner button.

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | Reported as the action's `component_id`. |
| `label` | `string` | The label. |

### `PluginBadgeSpec`

A badge an open hook shows in the message header.

| Member | Type | Description |
| --- | --- | --- |
| `text` | `string` | The text (at most `MAX_BADGE_TEXT` characters). |
| `level?` | `PluginNotificationLevel` | Its level (styling). |
| `icon?` | `string` | An icon of the theme's icon set. |
| `tooltip?` | `string` | A tooltip (at most `MAX_HOOK_TEXT` characters). |

### `banner`

A banner for an open hook's answer.

```ts
function banner( id: string, text: string, options:
```

### `badge`

A badge for an open hook's answer.

```ts
function badge( text: string, options:
```

### `PluginBannerAction`

A press on a button of a banner an open hook answered.

| Member | Type | Description |
| --- | --- | --- |
| `messageId` | `MessageId` | The message the banner is shown with. |
| `bannerId` | `string` | The banner's `id`. |
| `buttonId` | `string` | The button's `id`. |

### `PluginBannerActionHandler`

Called when the user presses a banner button.

```ts
type PluginBannerActionHandler = (action: PluginBannerAction) => void | Promise<void>;
```

## Composer

Composer actions and Markdown blocks (`composer`).

### `NailComposerApi`

`nail.composer`: composer actions, Markdown blocks and edits of the open draft, applied live (the user can undo them like typing). Needs `composer`. Edits work only on a draft a composer action of this plug-in ran on, for a short while after it ran.

`extends PluginComposerRegistry`

| Member | Type | Description |
| --- | --- | --- |
| `insert` | `(text: string, options?: PluginComposerEditOptions) => Promise<void>` | Inserts text at the cursor. |
| `replaceSelection` | `(text: string, options?: PluginComposerEditOptions) => Promise<void>` | Replaces the selection (inserts at the cursor without one). |
| `replaceAll` | `(text: string, options?: PluginComposerEditOptions) => Promise<void>` | Replaces the whole body. |

### `PluginComposerRegistry`

Composer registrations (`ctx.composer`).

| Member | Type | Description |
| --- | --- | --- |
| `registerAction` | `(name: string, handler: PluginActionHandler) => void` | Registers the handler of a composer action (`contributes.composerActions`). |
| `registerBlock` | `(name: string, renderer: PluginMarkdownBlockRenderer) => void` | Registers the renderer of a Markdown block (`contributes.markdownBlocks`). |

### `PluginComposerEditOptions`

Options of the composer edits.

| Member | Type | Description |
| --- | --- | --- |
| `draftId?` | `DraftId` | The draft (default: the draft of the latest composer action of this plug-in in the UI that ran it). |
| `format?` | `PluginTextFormat` | The format of the text (default `text`; HTML is sanitized). |

### `PluginActionHandler`

A composer, attachment or link action. `ctx.composer`, `ctx.attachment` or `ctx.link` says what it was invoked on; attachment actions also get the attachment's bytes.

```ts
type PluginActionHandler = ( ctx: PluginActionContext, bytes?: Uint8Array, ) => void | Promise<void>;
```

### `PluginActionContext`

What a composer / attachment / link action handler receives (plug-in-facing, camelCase).

| Member | Type | Description |
| --- | --- | --- |
| `action` | `string` | The action's name. |
| `composer?` | `PluginComposerContext` | For composer actions. |
| `attachment?` | `PluginAttachmentRef` | For attachment actions. |
| `link?` | `PluginLink` | For link actions. |

### `PluginComposerContext`

The open draft as a composer action gets it.

| Member | Type | Description |
| --- | --- | --- |
| `draftId` | `DraftId` | The draft. |
| `selection` | `ComposerSelection` | The selection. |
| `mode` | `string` | The draft's mode (`plain`, `html`, `markdown`). |

### `ComposerSelection`

The selection in the composer.

| Member | Type | Description |
| --- | --- | --- |
| `text` | `string` | The selected text (empty: only a cursor). |
| `start` | `number` | Start offset in the draft's source (UTF-16). |
| `end` | `number` | End offset (UTF-16; equals `start` without a selection). |

### `PluginMarkdownBlockRenderer`

Renders a Markdown block (a fenced block whose info string is `<plugin>.<block>`) to HTML and text.

```ts
type PluginMarkdownBlockRenderer = ( input: PluginMarkdownBlockInput, ) => PluginMarkdownBlockResult | Promise<PluginMarkdownBlockResult>;
```

### `PluginMarkdownBlockInput`

What a Markdown block renderer gets.

| Member | Type | Description |
| --- | --- | --- |
| `block` | `string` | The block's name. |
| `source` | `string` | The text inside the fence. |
| `purpose` | `PluginRenderPurpose` | Whether this is for the preview or the message being sent. |

### `PluginMarkdownBlockResult`

What a Markdown block renderer answers.

| Member | Type | Description |
| --- | --- | --- |
| `html` | `string` | HTML for the HTML part (sanitized; at most `MAX_BLOCK_HTML` bytes). |
| `text` | `string` | Text for the plain-text part. |

### `PluginRenderPurpose`

Why something is rendered.

```ts
type PluginRenderPurpose = "preview" | "send";
```

## Attachments and links

Attachment previews and actions (`attachments`) and link actions.

### `SdkAttachmentsRegistry`

The SDK's attachment registrations: previews may return JSX.

| Member | Type | Description |
| --- | --- | --- |
| `registerPreview` | `(name: string, renderer: SdkPreviewRenderer) => void` | Registers a preview renderer (`contributes.attachmentPreviews`). |
| `registerAction` | `(name: string, handler: PluginActionHandler) => void` | Registers an attachment action (`contributes.attachmentActions`). |

### `SdkPreviewRenderer`

A preview renderer in the SDK: it may return a `PluginPreview` (`previewTree`, `previewImage`) or JSX / nodes, which become a tree.

```ts
type SdkPreviewRenderer = ( attachment: PluginAttachmentRef, bytes: Uint8Array, ) => PluginPreview | UiChild | Promise<PluginPreview | UiChild>;
```

### `PluginAttachmentRef`

An attachment as an action or preview handler gets it (the bytes come separately, as a `Uint8Array`).

| Member | Type | Description |
| --- | --- | --- |
| `messageId` | `MessageId` | The message. |
| `index` | `number` | The attachment's index. |
| `name` | `string` | File name. |
| `mimeType` | `string` | MIME type. |
| `size` | `number` | Size (bytes). |

### `PluginPreview`

What an attachment preview renderer answers (tagged by `kind`).

**`kind: "tree"`**

| Member | Type | Description |
| --- | --- | --- |
| `tree` | `Array<PluginUiNode>` | The tree. |

**`kind: "image"`**

| Member | Type | Description |
| --- | --- | --- |
| `mimeType` | `string` | One of `PREVIEW_IMAGE_TYPES`. |
| `data` | `string` | The image, base64 (at most `MAX_PREVIEW_IMAGE` bytes decoded). |
| `alt?` | `string` | Alternative text. |

### `previewTree`

A preview drawn as a component tree; `content` may be JSX.

```ts
function previewTree(content: UiChild): PluginPreview
```

### `previewImage`

A preview image (PNG, JPEG, GIF or WebP; bytes or base64).

```ts
function previewImage( mimeType: (typeof PREVIEW_IMAGE_TYPES)[number], data: Uint8Array | ArrayBuffer | string, alt?: string, ): PluginPreview
```

### `PluginLinksRegistry`

Link registrations (`ctx.links`).

| Member | Type | Description |
| --- | --- | --- |
| `registerAction` | `(name: string, handler: PluginActionHandler) => void` | Registers a link action (`contributes.linkActions`). The reader does not offer link actions yet; test them with the test host. |

### `PluginLink`

A link as a link action gets it.

| Member | Type | Description |
| --- | --- | --- |
| `url` | `string` | The URL. |
| `text?` | `string` | The link's text. |
| `messageId?` | `MessageId` | The message it is in. |

## Background jobs

Declared in `contributes.jobs` (`background`).

### `PluginJobsRegistry`

Background job registrations (`ctx.jobs`).

| Member | Type | Description |
| --- | --- | --- |
| `register` | `(id: string, handler: PluginJobHandler) => void` | Registers the handler of a job (`contributes.jobs`). |

### `PluginJobHandler`

Runs a background job; the trigger says why.

```ts
type PluginJobHandler = (trigger: PluginJobTrigger) => void | Promise<void>;
```

### `PluginJobTrigger`

What started a job run (the handler's argument, camelCase).

**`kind: "schedule"`**

| Member | Type | Description |
| --- | --- | --- |
| `scheduledFor` | `string` | The scheduled time (RFC 3339). |

**`kind: "event"`**

| Member | Type | Description |
| --- | --- | --- |
| `event` | `PluginEventName` | The event. |
| `messageIds` | `Array<MessageId>` | Messages concerned (message events; ids the grant reaches). |

**`kind: "manual"`**

### `PluginEventName`

Events a job can subscribe to.

```ts
type PluginEventName = "message.received" | "message.sent" | "selection.changed" | "event.reminder" | "app.started";
```

## MCP tools

Declared in `contributes.mcpTools` (`mcp.expose`).

### `PluginMcpRegistry`

MCP tool registrations (`ctx.mcp`).

| Member | Type | Description |
| --- | --- | --- |
| `registerTool` | `(id: string, handler: PluginMcpToolHandler) => void` | Registers the handler of a tool (`contributes.mcpTools`). |

### `PluginMcpToolHandler`

Serves an MCP tool call (`contributes.mcpTools`): the agent's arguments in, a JSON result (or text) out. The result is handed to the agent as untrusted data.

```ts
type PluginMcpToolHandler = ( args: Record<string, JsonValue>, ) => JsonValue | undefined | void | Promise<JsonValue | undefined | void>;
```

## Bytes

The sandbox has no `TextDecoder`, `TextEncoder`, `atob` or `btoa`; these helpers fill in.

### `decodeText`

Decodes text: UTF-8 when the bytes are valid UTF-8, else Latin-1.

```ts
function decodeText(bytes: Uint8Array): string
```

### `decodeUtf8`

Decodes UTF-8 (a leading byte order mark is dropped; malformed sequences become U+FFFD, as `TextDecoder` does).

```ts
function decodeUtf8(bytes: Uint8Array): string
```

### `encodeUtf8`

Encodes a string as UTF-8 (lone surrogates become U+FFFD).

```ts
function encodeUtf8(text: string): Uint8Array
```

### `toBase64`

Base64 (standard alphabet, padded), e.g. for preview images.

```ts
function toBase64(bytes: Uint8Array): string
```

### `fromBase64`

Decodes base64 (standard or URL alphabet; whitespace and padding optional).

```ts
function fromBase64(text: string): Uint8Array
```

## Errors

Rejected calls carry a `code`.

### `PluginApiError`

How a rejected API call looks: an `Error` (name `NailError`) with a `code`. `forbidden` means the plug-in lacks the permission (or the grant does not cover the account); `too_large` that storage is over its quota; `rate_limited` that the plug-in sent too many notifications or timers.

`extends Error`

| Member | Type | Description |
| --- | --- | --- |
| `code` | `PluginApiErrorCode` | The error code. |

### `PluginApiErrorCode`

Codes of `PluginApiError`, the same as the codes of Nail's API errors.

```ts
type PluginApiErrorCode = | "forbidden" | "not_found" | "invalid_request" | "too_large" | "conflict" | "unsupported" | "rate_limited" | "network" | "unavailable" | "cancelled" | "internal" | (string & {});
```

### `isPluginApiError`

Whether `error` is an API error (optionally with this `code`), e.g. `isPluginApiError(e, "forbidden")` for a missing permission.

```ts
function isPluginApiError( error: unknown, code?: PluginApiErrorCode, ): error is PluginApiError
```

## Command context and selection

### `PluginCommandContext`

What a command handler receives (`ctx` in the design's example).

| Member | Type | Description |
| --- | --- | --- |
| `surface?` | `PluginCommandSurface` | Where it was invoked. |
| `selection` | `PluginSelection` | The selection. |
| `accountId?` | `AccountId` | The active account (real or virtual). |
| `folderId?` | `FolderId` | The active folder. |
| `draftId?` | `DraftId` | The draft open in the composer. |

### `PluginCommandSurface`

Where a command can be invoked from (the manifest's `contexts`) and where it was invoked.

```ts
type PluginCommandSurface = "list" | "reader" | "composer" | "global";
```

### `PluginSelection`

The messages a command acts on.

| Member | Type | Description |
| --- | --- | --- |
| `messageIds` | `Array<MessageId>` | Selected messages (the open one first when it is selected). |
| `openMessageId?` | `MessageId` | The message open in the reader. |
| `threadId?` | `ThreadId` | Its thread. |

## Dialogs and notifications

### `PluginDialogSpec`

A dialog as a plug-in asks for it (`nail.ui.showDialog(spec)`).

| Member | Type | Description |
| --- | --- | --- |
| `title` | `string` | The title. |
| `body` | `Array<PluginUiNode>` | The content. |
| `buttons?` | `Array<PluginDialogButton>` | The buttons that close the dialog (none: only the close button). |
| `modal?` | `boolean` | Whether the rest of the UI is blocked while it is open (default `true`); a non-modal dialog is a floating window. |
| `size?` | `PluginDialogSize` | Its size. |

### `PluginDialogButton`

A dialog button.

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | Reported as `button`. |
| `label` | `string` | The label. |
| `primary?` | `boolean` | The default button (Enter); it needs every `required` value. |

### `PluginDialogSize`

The size of a dialog.

```ts
type PluginDialogSize = "small" | "medium" | "large";
```

### `PluginDialogResult`

What `nail.ui.showDialog` resolves to.

| Member | Type | Description |
| --- | --- | --- |
| `button` | `string \| null` | The button that closed the dialog; `null` when it was dismissed (Escape, the close button, the plug-in stopped). |
| `values` | `{ [key in string]: JsonValue }` | The values of the input nodes by id (see `ui`). |

### `PluginNotificationSpec`

A notification as a plug-in asks for it (`nail.ui.notify`). A plain string is accepted by the SDK as `{title}`.

| Member | Type | Description |
| --- | --- | --- |
| `title` | `string` | The text. |
| `body?` | `string` | More text. |
| `level?` | `PluginNotificationLevel` | The level. |
| `system?` | `boolean` | Also as an OS notification (desktop) / Web Notification (remote UI), subject to the notification settings' quiet hours; otherwise an in-app toast only. |

### `PluginNotificationLevel`

The level of a notification.

```ts
type PluginNotificationLevel = "info" | "success" | "warning" | "error";
```

## Mail

What `nail.mail` returns.

### `PluginMessage`

A message as a plug-in with `mail.read` sees it (`nail.mail.get`, `nail.mail.list`, `message-list` rows): headers, no body.

| Member | Type | Description |
| --- | --- | --- |
| `id` | `MessageId` | The message. |
| `uri` | `string` | `nail://messages/<id>` (`message_uri`). |
| `accountId` | `AccountId` | Its account. |
| `folderId` | `FolderId` | Its folder. |
| `threadId?` | `ThreadId` | Its thread. |
| `subject` | `string` | The subject (empty when missing). |
| `from?` | `Address` | The sender. |
| `to` | `Array<Address>` | `To` recipients. |
| `cc` | `Array<Address>` | `Cc` recipients. |
| `date` | `string` | RFC 3339 date. |
| `flags` | `Flags` | Flags. |
| `tags` | `Array<string>` | Tags (keywords). |
| `hasAttachments` | `boolean` | Whether it has attachments. |
| `encrypted` | `boolean` | Whether it is end-to-end encrypted (its body is then never given to a preview plug-in). |

### `PluginMessageBody`

A message body as a plug-in with `mail.read.body` sees it (`nail.mail.getBody`). Never of end-to-end encrypted mail in the preview.

| Member | Type | Description |
| --- | --- | --- |
| `id` | `MessageId` | The message. |
| `text` | `string` | The plain-text body (or the text rendering of the HTML body). |
| `html?` | `string` | The sanitized HTML body, if the message has one. |
| `attachments` | `Array<PluginAttachment>` | The attachments (names and sizes, no content). |

### `PluginAttachment`

An attachment as a plug-in sees it.

| Member | Type | Description |
| --- | --- | --- |
| `name` | `string` | The file name. |
| `mimeType` | `string` | The MIME type. |
| `size` | `number` | The size in bytes. |

### `Address`

A mail address with an optional display name.

| Member | Type | Description |
| --- | --- | --- |
| `name` | `string \| null` | Display name, e.g. "Ada Lovelace". `null` when the header has none. |
| `email` | `string` | The address itself, e.g. "`ada@example.org`". |

### `Flags`

Standard IMAP system flags of a message.

| Member | Type | Description |
| --- | --- | --- |
| `seen` | `boolean` | `\Seen`: the message has been read. |
| `flagged` | `boolean` | `\Flagged`: starred / flagged for follow-up. |
| `answered` | `boolean` | `\Answered`: a reply was sent. |
| `draft` | `boolean` | `\Draft`: the message is a draft. |

### `MessageId`

Identifies a message in the local store. Stable across restarts.

```ts
type MessageId = string;
```

### `ThreadId`

Identifies a conversation thread. Threads can span folders and, in a virtual account, accounts.

```ts
type ThreadId = string;
```

### `AccountId`

Identifies an account, real or virtual. For accounts configured in `accounts.kdl` it is the name written there (`account "work"`); the built-in "All accounts" virtual account is `ALL`.

```ts
type AccountId = string;
```

### `FolderId`

Identifies a folder (mailbox) within an account.

```ts
type FolderId = string;
```

### `JsonValue`

```ts
type JsonValue = number | string | boolean | Array<JsonValue> | { [key in string]: JsonValue } | null;
```

## Calendars

What `nail.calendar` returns (needs `calendar.read`).

### `PluginCalendarEvent`

An event occurrence as a plug-in with `calendar.read` sees it (`nail.calendar.events`; M3a).

| Member | Type | Description |
| --- | --- | --- |
| `eventId` | `EventId` | The event (series). |
| `calendarId` | `CalendarId` | Its calendar. |
| `title` | `string` | Title. |
| `start` | `string` | Start (RFC 3339 UTC; all-day events at local midnight). |
| `end` | `string` | End (RFC 3339 UTC). |
| `allDay` | `boolean` | An all-day event. |
| `location?` | `string` | Location, if any. |
| `recurring` | `boolean` | Part of a recurring series. |

### `EventId`

Identifies a calendar event, the whole series of a recurring one (`CalendarEvent`). Opaque; not the iCalendar `UID`.

```ts
type EventId = string;
```

### `CalendarId`

Identifies a calendar (`Calendar`). Task lists are calendars that hold tasks (CalDAV keeps both in one collection). Opaque, made by the core.

```ts
type CalendarId = string;
```

## JSX and trees

### `toNodes`

Flattens JSX children or content into a list of nodes: arrays are flattened, `null` / `undefined` / booleans / empty strings dropped, other strings and numbers become `text` nodes.

```ts
function toNodes(content: UiChild): PluginUiNode[]
```

### `UiChild`

What may appear as JSX children.

```ts
type UiChild = PluginUiNode | string | number | boolean | null | undefined | readonly UiChild[];
```

### `checkTree`

Checks a tree; returns every issue found (empty when it looks fine).

```ts
function checkTree(nodes: readonly PluginUiNode[], root = "body"): TreeIssue[]
```

### `TreeIssue`

A problem found by `checkTree`.

| Member | Type | Description |
| --- | --- | --- |
| `path` | `string` | Where: `body[1].children[0]`. |
| `message` | `string` | What (English, for logs and test failures). |

## Testing plug-ins

From `@nail/plugin-api/testing`.

### `createTestHost`

Creates an in-memory host.

```ts
function createTestHost(options: TestHostOptions =
```

### `TestHostOptions`

Options of `createTestHost`.

| Member | Type | Description |
| --- | --- | --- |
| `manifest?` | `PluginManifest` | The plug-in's manifest: gives the id, version and (unless `permissions` is set) the granted permissions; commands, panels, hooks and extensions must then be declared in it to be registered, and hook budgets come from it. |
| `permissions?` | `PluginPermission[]` | The granted permissions (default: the manifest's, else `ALL_FIXED_PERMISSIONS`). |
| `accounts?` | `string[]` | The accounts the account-limited permissions are granted for (default: every account). |
| `messages?` | `PluginMessage[]` | Messages `nail.mail` can see. |
| `bodies?` | `Record<string, PluginMessageBody>` | Bodies by message id. |
| `events?` | `PluginCalendarEvent[]` | Event occurrences `nail.calendar.events` can see. |
| `calendars?` | `PluginCollection[]` | Calendars the plug-in may write to (default: one, `calendar`). |
| `taskLists?` | `PluginCollection[]` | Task lists (default: one, `tasks`). |
| `addressBooks?` | `PluginCollection[]` | Address books (default: one, `contacts`). |
| `contacts?` | `PluginContact[]` | Contacts `nail.contacts.find` can see. |
| `settings?` | `Record<string, JsonValue>` | Setting values. |
| `locale?` | `string` | Nail's language for `nail.l10n` (default `en`). |
| `catalogs?` | `Record<string, string>` | The plug-in's catalogs: language tag → `.ftl` text (`loadCatalogs` in `@nail/plugin-api/tools` reads them). |
| `dialog?` | `DialogResponder` | Answers dialogs (default: dismissed, `{button: null, values: {}}`). |
| `confirm?` | `( confirmation: Omit<TestConfirmation, "decision">, ) => TestDecision \| Promise<TestDecision>` | Answers confirmations (default: approve). |
| `policies?` | `Partial<Record<PluginActionKind, "ask" \| "allow">>` | Changed policies: `allow` for a kind that asks by default, `ask` for one that does not (destructive kinds always ask). |
| `sendVetoes?` | `(draft: TestDraft) => string[]` | Other plug-ins' before-send vetoes for `nail.mail.send` (reasons; empty = none). |
| `fetch?` | `(url: string, request: PluginHttpRequest) => TestHttpResponse \| Promise<TestHttpResponse>` | Answers `nail.http.fetch` for allowed hosts (default 404). |
| `storageQuota?` | `number` | Storage quota in bytes (default 5 MiB). |
| `hookTimeoutMs?` | `Partial<Record<PluginHookKind, number>>` | Hook budgets in ms (default: the manifest's `timeoutMs`, else Nail's defaults). |
| `jobTimeoutMs?` | `number` | Wall-clock limit of a job run in ms (default 60 s). |

### `TestHost`

The test host (see `createTestHost`).

| Member | Type | Description |
| --- | --- | --- |
| `api` | `NailApi` | The `nail` object. |
| `dialogs` | `PluginDialogSpec[]` | Dialogs shown, in order. |
| `notifications` | `PluginNotificationSpec[]` | Notifications shown, in order. |
| `panelUpdates` | `{ name: string; tree: PluginUiNode[] }[]` | Panel updates (`nail.ui.updatePanel`), in order. |
| `logs` | `TestLogLine[]` | Log lines. |
| `storage` | `Map<string, JsonValue>` | The storage contents. |
| `writes` | `TestWrite[]` | Every write, in order (done, denied, expired or vetoed). |
| `confirmations` | `TestConfirmation[]` | Every confirmation the user was asked, in order. |
| `composerEdits` | `{ draftId: DraftId; edit: ComposerEdit }[]` | Composer edits, in order. |
| `messages` | `Map<string, PluginMessage>` | The messages (updated by `nail.mail.update` and arrival actions). |
| `drafts` | `Map<string, TestDraft>` | Drafts created. |
| `tasks` | `Map<string, TestTask>` | Tasks created. |
| `calendarEvents` | `Map<string, TestEvent>` | Events created. |
| `contactsById` | `Map<string, PluginContact>` | Contacts created or changed. |
| `commandNames` | `() => string[]` | Registered command names. |
| `panelNames` | `() => string[]` | Registered panel names. |
| `hookNames` | `() => PluginHookKind[]` | Registered hooks. |
| `extensionNames` | `() => { composerActions: string[]; markdownBlocks: string[]; attachmentPreviews: string[]; attachmentActions: string[]; linkActions: string[]; jobs: string[]; mcpTools: string[]; }` | Registered extensions by kind. |
| `install` | `() => () => void` | Installs `api` as `globalThis.nail`; returns a function restoring the previous value. |
| `activationContext` | `() => PluginActivationContext` | The context the host passes to `activate`. |
| `runCommand` | `(name: string, ctx?: Partial<PluginCommandContext>) => Promise<void>` | Runs a registered command like a click in the UI would. |
| `renderPanel` | `(name: string, ctx?: Partial<PluginCommandContext>) => Promise<PluginUiNode[]>` | Renders a registered panel (checking the tree). |
| `clickPanelButton` | `(name: string, componentId: string, values?: Record<string, JsonValue>) => Promise<void>` | Clicks a `button` node in a panel. |
| `changeSettings` | `(values: Record<string, JsonValue>) => void` | Changes settings and calls the `onChange` listeners. |
| `runArrival` | `(message: PluginMessage \| PluginHookMessage) => Promise<TestHookRun<PluginArrivalResult>>` | Delivers a new message to the arrival hook; valid actions are applied to `messages`. |
| `runBeforeSend` | `(message: PluginOutgoingMessage) => Promise<TestHookRun<PluginBeforeSendResult>>` | Hands an outgoing message to the before-send hook. |
| `runOpen` | `(message: PluginMessage \| PluginHookMessage) => Promise<TestHookRun<PluginOpenResult>>` | Opens a message: runs the open hook. |
| `clickBanner` | `(action: PluginBannerAction) => Promise<void>` | Presses a banner button. |
| `runComposerAction` | `(name: string, composer?: Partial<PluginComposerContext>) => Promise<void>` | Runs a composer action on a draft (selection and mode default to empty / `markdown`). |
| `renderMarkdownBlock` | `(name: string, source: string, purpose?: PluginRenderPurpose) => Promise<PluginMarkdownBlockResult>` | Renders a Markdown block (checking the answer). |
| `renderPreview` | `(name: string, attachment: Partial<PluginAttachmentRef> & { name: string }, bytes: Uint8Array \| string) => Promise<PluginPreview>` | Renders an attachment preview (checking size, the tree or the image). |
| `runAttachmentAction` | `(name: string, attachment: Partial<PluginAttachmentRef> & { name: string }, bytes: Uint8Array \| string) => Promise<void>` | Runs an attachment action with the attachment's bytes. |
| `runLinkAction` | `(name: string, link: PluginLink) => Promise<void>` | Runs a link action. |
| `runJob` | `(id: string, trigger?: PluginJobTrigger) => Promise<TestJobRun>` | Runs a background job (manual trigger unless given). |
| `callMcpTool` | `(id: string, args?: Record<string, JsonValue>) => Promise<JsonValue \| undefined>` | Calls an MCP tool like an agent would; resolves with its result. |

### `TestHookRun`

How a hook call ended, as Nail counts it.

| Member | Type | Description |
| --- | --- | --- |
| `outcome` | `"ok" \| "vetoed" \| "failed" \| "timed-out" \| "skipped"` | `ok` (answered, maybe nothing), `vetoed` (before-send veto), `failed` (threw or answered something invalid), `timed-out` (over its budget) or `skipped` (not registered, not permitted, account not granted). |
| `result?` | `T` | The answer (valid answers only). |
| `problems` | `string[]` | Why the answer was refused. |
| `error?` | `string` | The error it threw, or why it was skipped. |

### `TestJobRun`

How a job run ended.

| Member | Type | Description |
| --- | --- | --- |
| `outcome` | `"ok" \| "error" \| "timeout"` | `ok`, `error` (threw) or `timeout` (over the wall-clock limit). |
| `error?` | `string` | The error. |

### `TestConfirmation`

A confirmation the test host's user was asked.

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | Its id (`confirmation-1`, …). |
| `kind` | `PluginActionKind` | The kind of write. |
| `operation` | `string` | The API call (`nail.mail.send`). |
| `summary` | `string` | What it would do (English, for tests). |
| `targets` | `string[]` | The items concerned (message ids, draft id, …). |
| `canAlwaysAllow` | `boolean` | Whether "always allow" could be offered. |
| `decision` | `TestDecision` | How the test's user decided. |

### `TestDecision`

How the scripted user answers a confirmation.

```ts
type TestDecision = "approve" | "deny" | "expire";
```

### `TestWrite`

A write the plug-in made (done or not).

| Member | Type | Description |
| --- | --- | --- |
| `operation` | `string` | The API call (`nail.tasks.create`, `hook.arrival`). |
| `kind` | `PluginActionKind` | The kind of write. |
| `input` | `JsonValue` | The arguments as passed. |
| `result` | `PluginWriteResult` | How it ended. |

## Manifest types

The types of `manifest.json` (see the manifest reference for the rules).

### `PluginManifest`

A plug-in's `manifest.json`.

| Member | Type | Description |
| --- | --- | --- |
| `id` | `PluginId` | The plug-in id (`is_plugin_id`), unique per installation. |
| `name` | `string` | Display name (or `%fluent-id%`). |
| `version` | `string` | Semantic version (`is_plugin_version`). |
| `apiVersion` | `string` | The plug-in API major version it is written for (`"1"`). |
| `main` | `string` | The bundled JavaScript entry point, relative to the package root (`dist/index.js`). |
| `description?` | `string` | One-paragraph description (or `%fluent-id%`). |
| `author?` | `string` | The author. |
| `homepage?` | `string` | A web page about the plug-in (shown as text, `https:` only). |
| `permissions?` | `Array<PluginPermission>` | The permissions it needs. |
| `contributes?` | `PluginContributions` | What it adds to Nail. |

### `PluginContributions`

The contributions of a manifest.

| Member | Type | Description |
| --- | --- | --- |
| `commands?` | `Array<PluginCommandDef>` | Named actions (`plugin.<id>.<command>` in `layout.kdl`). |
| `placements?` | `Array<PluginPlacement>` | Suggested toolbar / menu items and panel positions. |
| `keys?` | `Array<PluginKeyDef>` | Suggested key bindings. |
| `panels?` | `Array<PluginPanelDef>` | Panels placeable in the layout (`plugin-panel "<id>.<panel>"`). |
| `settings?` | `Array<PluginSettingDef>` | The settings schema; becomes a generated settings page. |
| `locales?` | `{ [key in string]: string }` | The plug-in's Fluent catalogs: language tag (`en`, `de`, `pt-BR`) → `.ftl` file in the package (also `nail.l10n` at run time). |
| `hooks?` | `Array<PluginHookDef>` | Mail hooks, at most one per kind (M4a; `hooks.*`). |
| `composerActions?` | `Array<PluginComposerActionDef>` | Composer actions (M4a; `composer`). |
| `markdownBlocks?` | `Array<PluginMarkdownBlockDef>` | Markdown block renderers (M4a; `composer`). |
| `attachmentPreviews?` | `Array<PluginAttachmentPreviewDef>` | Attachment preview renderers (M4a; `attachments`). |
| `attachmentActions?` | `Array<PluginAttachmentActionDef>` | Attachment menu actions (M4a; `attachments`). |
| `linkActions?` | `Array<PluginLinkActionDef>` | Link actions (M4a; no permission: they get only the link the user picked). Not offered in the reader yet, so users cannot pick them in this version. |
| `jobs?` | `Array<PluginJobDef>` | Background jobs (M4a; `background`). |
| `mcpTools?` | `Array<PluginMcpToolDef>` | Tools offered to MCP clients (M4a; `mcp.expose`). |

### `PluginCommandDef`

A command.

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | The command name (`is_contribution_name`). |
| `title` | `string` | The label (or `%fluent-id%`). |
| `icon?` | `string` | Icon name from the theme's icon set. |
| `description?` | `string` | A tooltip / palette description (or `%fluent-id%`). |
| `contexts?` | `Array<PluginCommandSurface>` | Where it can be invoked; empty = everywhere. |

### `PluginPlacement`

A suggested placement. Exactly one of `toolbar`, `menu` and `panel` is set; at most one of `after` and `before`. • `{"toolbar": "reader", "command": "x", "after": "mail.forward"}` — an item in a toolbar of `layout.kdl`. • `{"menu": "context:message-list.row", "command": "x"}` — an item in a menu (`app` or `context:<part>`). • `{"panel": "due", "after": "message-view"}` — the panel after (or before) the first widget of that kind; at the end of the main window when there is none.

| Member | Type | Description |
| --- | --- | --- |
| `toolbar?` | `string` | A toolbar name. |
| `menu?` | `string` | A menu name. |
| `panel?` | `string` | A panel of this plug-in. |
| `command?` | `string` | The command (toolbar and menu placements). |
| `after?` | `string` | Place after this action (toolbar / menu) or widget kind (panel). |
| `before?` | `string` | Place before this action or widget kind. |

### `PluginKeyDef`

A suggested key binding.

| Member | Type | Description |
| --- | --- | --- |
| `key` | `string` | Key sequence as in `layout.kdl` (`shift+f`, `ctrl+alt+r`, `g f`). |
| `command` | `string` | The command. |
| `context?` | `string` | The key context (`KEY_CONTEXTS`); default `global`. |

### `PluginPanelDef`

A panel.

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | The panel name (`is_contribution_name`). |
| `title` | `string` | Its title (or `%fluent-id%`). |
| `icon?` | `string` | Icon name. |

### `PluginSettingDef`

One setting of the generated settings page. Values are JSON: string, integer, boolean, or one of the options' values.

**`type: "string"`**

| Member | Type | Description |
| --- | --- | --- |
| `key` | `string` | The setting's key (`is_contribution_name`). |
| `title` | `string` | Its label (or `%fluent-id%`). |
| `description?` | `string` | Help text (or `%fluent-id%`). |
| `default?` | `string` | The default (empty when absent). |
| `multiline?` | `boolean` | A multi-line field. |
| `maxLength?` | `number` | Longest value (characters; at most 10 000). |

**`type: "number"`**

| Member | Type | Description |
| --- | --- | --- |
| `key` | `string` | The setting's key. |
| `title` | `string` | Its label. |
| `description?` | `string` | Help text. |
| `default?` | `number` | The default (0 when absent, or `min`). |
| `min?` | `number` | Smallest value. |
| `max?` | `number` | Largest value. |

**`type: "boolean"`**

| Member | Type | Description |
| --- | --- | --- |
| `key` | `string` | The setting's key. |
| `title` | `string` | Its label. |
| `description?` | `string` | Help text. |
| `default?` | `boolean` | The default. |

**`type: "select"`**

| Member | Type | Description |
| --- | --- | --- |
| `key` | `string` | The setting's key. |
| `title` | `string` | Its label. |
| `description?` | `string` | Help text. |
| `options` | `Array<PluginSelectOption>` | The choices (labels may be `%fluent-id%`). |
| `default?` | `string` | The default (the first option when absent). |

### `PluginSelectOption`

A choice of a `select` node (also used by `select` settings).

| Member | Type | Description |
| --- | --- | --- |
| `value` | `string` | The value reported when chosen. |
| `label` | `string` | The text shown. |

### `PluginPermission`

A plug-in permission (the preview's and, since M4a, the writes, hooks and extension points).

```ts
type PluginPermission = "mail.read" | "mail.read.body" | "calendar.read" | "ui.dialogs" | "notifications" | "storage" | "mail.organize" | "mail.send" | "calendar.write" | "contacts.write" | "tasks.write" | "hooks.arrival" | "hooks.beforeSend" | "hooks.open" | "composer" | "attachments" | "background" | "mcp.expose" | `network:${string}`;
```

### `PluginHookDef`

A hook in the manifest (`contributes.hooks`; at most one per kind).

| Member | Type | Description |
| --- | --- | --- |
| `hook` | `PluginHookKind` | Which hook. |
| `filter?` | `string` | Only messages matching this query of Nail's search language (`from:@acme.example has:attachment`); arrival and open hooks only. |
| `timeoutMs?` | `number` | A smaller budget than the default (ms, at least `MIN_HOOK_TIMEOUT_MS`, at most the kind's default … up to `MAX_HOOK_TIMEOUT_MS`). |

### `PluginHookKind`

The kind of a mail hook.

```ts
type PluginHookKind = "arrival" | "beforeSend" | "open";
```

### `PluginComposerActionDef`

A composer action (`contributes.composerActions`).

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | The name (`is_contribution_name`); the UI's action id is `plugin_extension_id`. |
| `title` | `string` | The label (or `%fluent-id%`). |
| `icon?` | `string` | An icon of the theme's icon set. |
| `description?` | `string` | A tooltip (or `%fluent-id%`). |

### `PluginMarkdownBlockDef`

A Markdown block renderer (`contributes.markdownBlocks`).

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | The block name; fences use `markdown_block_info` (`<plugin>.<block>`). |
| `title` | `string` | What the composer's block menu shows (or `%fluent-id%`). |
| `template?` | `string` | Text inserted as the block's body when picked from the menu (or `%fluent-id%`). |

### `PluginAttachmentPreviewDef`

An attachment preview renderer (`contributes.attachmentPreviews`).

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | The renderer's name. |
| `title` | `string` | What the preview pane names it (or `%fluent-id%`). |
| `mimeTypes?` | `Array<string>` | MIME types it renders (`text/csv`, `application/pdf`; `type/*` allowed). |
| `extensions?` | `Array<string>` | File extensions it renders, lowercase without the dot (`csv`). |
| `maxBytes?` | `number` | Largest attachment it accepts (bytes; at most `MAX_PREVIEW_INPUT`). |

### `PluginAttachmentActionDef`

An action in the attachment menu (`contributes.attachmentActions`).

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | The action's name. |
| `title` | `string` | The label (or `%fluent-id%`). |
| `icon?` | `string` | An icon. |
| `mimeTypes?` | `Array<string>` | MIME types it applies to (empty with no extensions = every attachment). |
| `extensions?` | `Array<string>` | Extensions it applies to. |

### `PluginLinkActionDef`

An action for links (`contributes.linkActions`). The reader does not offer link actions yet (links sit in its sandboxed frame): they are declared and testable, but users cannot pick them in this version.

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | The action's name. |
| `title` | `string` | The label (or `%fluent-id%`). |
| `icon?` | `string` | An icon. |
| `hosts?` | `Array<string>` | Host patterns it applies to (`jira.example.com`, `*.example.com`; empty = every link). |

### `PluginJobDef`

A job in the manifest (`contributes.jobs`).

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | The job's name (`is_contribution_name`). |
| `title?` | `string` | What the plug-in manager calls it (or `%fluent-id%`). |
| `schedule?` | `string` | When it runs: five cron fields in local time (`0,30 * * * *`, `0 8 * * 1-5`; steps like `/15` allowed) or `@hourly` / `@daily` / `@weekly` (`parse`). |
| `events?` | `Array<PluginEventName>` | Events that run it. |

### `PluginMcpToolDef`

A tool offered to MCP clients (`contributes.mcpTools`, needs `mcp.expose`).

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | The tool's name within the plug-in (the MCP name is `plugin_mcp_tool_name`, at most `MAX_MCP_TOOL_NAME` characters). |
| `description` | `string` | What it does, for the agent (English is fine: agents read it; or `%fluent-id%`, given in Nail's language; at most `MAX_MCP_TOOL_DESCRIPTION` characters). |
| `inputSchema?` | `JsonValue` | The JSON Schema of its arguments (an object schema; default: no arguments). |
| `scopes?` | `Array<McpScope>` | The scopes an MCP client needs to see and call it (default `read`). Writes the plug-in makes while serving the call need the plug-in's own permissions and follow its action policies. |

### `McpScope`

What a client may do. Reading message content needs `read`; raw source, attachment bytes, encrypted content and the UI context each need their own opt-in scope on top.

```ts
type McpScope = "read" | "raw-source" | "attachments" | "encrypted" | "context" | "organize" | "permanent-delete" | "draft" | "send" | "calendar" | "pim" | "rules";
```

<!-- END GENERATED -->
