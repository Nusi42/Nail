# Plug-in API reference

The API of the plug-in preview (`apiVersion` `"1"`), from the
`@nail/plugin-api` package. Everything below is generated from the SDK's
types, which are generated from Nail's own contract, so it matches what
Nail does.

```ts
import { definePlugin, nail, isPluginApiError } from "@nail/plugin-api";
```

A few rules hold for every call:

- Calls that reach Nail return promises. `nail.settings` and `nail.log` are
  synchronous.
- Every call is checked against the plug-in's [permissions](permissions.md);
  without one it rejects with an error whose `code` is `forbidden`.
- JSON is exchanged in camelCase (`messageIds`, `hasAttachments`).
- An error that a command handler, panel or `activate` does not catch
  stops the plug-in.
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
| `activate` | `(ctx: PluginContext) => void \| Promise<void>` | Called once when the plug-in starts: register commands and panels. |
| `deactivate?` | `() => void` | Called when the plug-in stops; synchronous (the host does not wait). |

### `PluginContext`

The context `activate` receives: the host's, with JSX panels.

| Member | Type | Description |
| --- | --- | --- |
| `plugin` | `PluginIdentity` | The plug-in's id and version. |
| `commands` | `NailCommandsApi` | Command handlers. |
| `panels` | `{ register(name: string, panel: PanelProvider \| PanelRender): void; }` | Panel providers; `render` may return JSX. |
| `settings` | `NailSettingsApi` | The plug-in's settings. |

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
| `mail` | `NailMailApi` | Mail (read only in the preview). |
| `calendar` | `NailCalendarApi` | Calendars (read only; `calendar.read`). |
| `ui` | `SdkUiApi` | Commands, panels, dialogs and notifications. |
| `commands` | `NailCommandsApi` | Command handlers. |
| `storage` | `NailStorageApi` | Key-value storage. |
| `http` | `NailHttpApi` | HTTP to declared hosts. |
| `settings` | `NailSettingsApi` | The plug-in's settings. |
| `log` | `NailLogApi` | The plug-in's log. |

### `NailMailApi`

`nail.mail`: read access to mail. Headers need `mail.read`, bodies `mail.read.body`, each for the message's account.

| Member | Type | Description |
| --- | --- | --- |
| `get` | `(id: MessageId) => Promise<PluginMessage>` | One message's headers. |
| `getBody` | `(id: MessageId) => Promise<PluginMessageBody>` | One message's body. Always rejects with `forbidden` for end-to-end encrypted mail. |
| `search` | `(query: string, options?: PluginMailSearchOptions) => Promise<PluginMessage[]>` | Messages matching a query of Nail's search language (`from:ada has:attachment`), newest first, in the granted accounts. Message text is searched only where `mail.read.body` is granted: elsewhere free text matches sender, recipients and subject, and a query with `body:` or `filename:` rejects with `forbidden` when no account allows it. End-to-end encrypted mail is left out while Nail indexes its decrypted text. |
| `thread` | `(threadId: ThreadId) => Promise<PluginMessage[]>` | The messages of a thread, oldest first. |

### `PluginMailSearchOptions`

Options of `NailMailApi.search`.

| Member | Type | Description |
| --- | --- | --- |
| `limit?` | `number` | At most this many messages (the host clamps it). |

### `NailCalendarApi`

`nail.calendar`: read access to the user's calendars. Needs `calendar.read`.

| Member | Type | Description |
| --- | --- | --- |
| `events` | `(from: string, to: string, options?: PluginCalendarEventsOptions) => Promise<PluginCalendarEvent[]>` | The event occurrences overlapping `[from, to)` (RFC 3339 times, at most 92 days apart) in every calendar the user shows, by start. Recurring events come as one entry per occurrence. |

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
| `body?` | `string` | A text body (serialize JSON yourself); not for `GET` / `HEAD`. |

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
| `body` | `string` | The body as text. |
| `text` | `() => Promise<string>` | The body as text. |
| `json` | `<T = JsonValue>() => Promise<T>` | The body parsed as JSON. |

### `NailSettingsApi`

`nail.settings`: the values of the manifest's settings schema, as the user set them on the plug-in's generated settings page (defaults otherwise). Synchronous; no permission needed.

| Member | Type | Description |
| --- | --- | --- |
| `get` | `<T extends JsonValue = JsonValue>(key: string) => T \| undefined` | One setting's value (`undefined` for a key the schema does not have). |
| `all` | `() => Record<string, JsonValue>` | Every setting's value. |
| `onChange` | `(listener: (values: Record<string, JsonValue>) => void) => void` | Called with every value whenever the user changes a setting. |

### `NailLogApi`

`nail.log`: lines in the plug-in's log (plug-in manager, `nail plugin dev`). `console.*` writes there too. No permission needed.

| Member | Type | Description |
| --- | --- | --- |
| `debug` | `(...args: unknown[]) => void` | A debug line. |
| `info` | `(...args: unknown[]) => void` | An information line. |
| `warn` | `(...args: unknown[]) => void` | A warning. |
| `error` | `(...args: unknown[]) => void` | An error. |

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
| `email` | `string` | The address itself, e.g. "ada@example.org". |

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
| `manifest?` | `PluginManifest` | The plug-in's manifest: gives the id, version and (unless `permissions` is set) the granted permissions; commands and panels must then be declared in it to be registered. |
| `permissions?` | `PluginPermission[]` | The granted permissions (default: the manifest's, else `ALL_FIXED_PERMISSIONS`). |
| `messages?` | `PluginMessage[]` | Messages `nail.mail` can see. |
| `bodies?` | `Record<string, PluginMessageBody>` | Bodies by message id. |
| `events?` | `PluginCalendarEvent[]` | Event occurrences `nail.calendar.events` can see. |
| `settings?` | `Record<string, JsonValue>` | Setting values. |
| `dialog?` | `DialogResponder` | Answers dialogs (default: dismissed, `{button: null, values: {}}`). |
| `fetch?` | `(url: string, request: PluginHttpRequest) => TestHttpResponse \| Promise<TestHttpResponse>` | Answers `nail.http.fetch` for allowed hosts (default 404). |
| `storageQuota?` | `number` | Storage quota in bytes (default 5 MiB). |

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
| `commandNames` | `() => string[]` | Registered command names. |
| `panelNames` | `() => string[]` | Registered panel names. |
| `install` | `() => () => void` | Installs `api` as `globalThis.nail`; returns a function restoring the previous value. |
| `activationContext` | `() => PluginActivationContext` | The context the host passes to `activate`. |
| `runCommand` | `(name: string, ctx?: Partial<PluginCommandContext>) => Promise<void>` | Runs a registered command like a click in the UI would. |
| `renderPanel` | `(name: string, ctx?: Partial<PluginCommandContext>) => Promise<PluginUiNode[]>` | Renders a registered panel (checking the tree). |
| `clickPanelButton` | `(name: string, componentId: string, values?: Record<string, JsonValue>) => Promise<void>` | Clicks a `button` node in a panel. |
| `changeSettings` | `(values: Record<string, JsonValue>) => void` | Changes settings and calls the `onChange` listeners. |

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
| `locales?` | `{ [key in string]: string }` | The plug-in's Fluent catalogs: language tag (`en`, `de`, `pt-BR`) → `.ftl` file in the package. |

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

A suggested placement. Exactly one of `toolbar`, `menu` and `panel` is set; at most one of `after` and `before`. * `{"toolbar": "reader", "command": "x", "after": "mail.forward"}` — an item in a toolbar of `layout.kdl`. * `{"menu": "context:message-list.row", "command": "x"}` — an item in a menu (`app` or `context:<part>`). * `{"panel": "due", "after": "message-view"}` — the panel after (or before) the first widget of that kind; at the end of the main window when there is none.

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

A permission of the preview API.

```ts
type PluginPermission = "mail.read" | "mail.read.body" | "calendar.read" | "ui.dialogs" | "notifications" | "storage" | `network:${string}`;
```

<!-- END GENERATED -->
