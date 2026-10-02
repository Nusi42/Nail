/**
 * The plug-in API (`apiVersion` "1"): what a plug-in bundle sees at run
 * time. The host (`crates/nail-plugins`, its `prelude.js`) installs a
 * frozen global `nail` object implementing {@link NailApi}, plus
 * `console`, `setTimeout`, `clearTimeout` and `queueMicrotask` (typed by
 * `@nail/plugin-api/globals`), then loads the bundle and calls its default
 * export's `activate` ({@link PluginDefinition}).
 *
 * Calls that reach Nail return promises and pass the permission gate
 * first: a call without the matching permission rejects with a
 * {@link PluginApiError} whose `code` is `"forbidden"`. An error a command
 * handler, panel or `activate` does not catch stops the plug-in, so catch
 * the errors you expect.
 *
 * M4a (ADR 0079) adds the writes (`nail.mail.update` … `nail.tasks.*`,
 * each through Nail's operation queue and the plug-in's action policies:
 * a write may wait for the user's confirmation and resolves with a
 * {@link PluginWriteResult}), composer edits, `nail.l10n`, binary
 * `nail.http.fetch` bodies, and the handlers of hooks, composer /
 * attachment / link actions, attachment previews, background jobs and MCP
 * tools ({@link PluginActivationContext}).
 */
import type {
  CalendarId,
  ComposerTextFormat,
  DraftId,
  EventId,
  FolderId,
  IdentityId,
  JsonValue,
  MessageId,
  PluginActionContext,
  PluginArrivalResult,
  PluginAttachmentRef,
  PluginBeforeSendResult,
  PluginCommandContext,
  PluginDialogResult,
  PluginDialogSpec,
  PluginCalendarEvent,
  PluginHookKind,
  PluginId,
  PluginJobTrigger,
  PluginMarkdownBlockInput,
  PluginMarkdownBlockResult,
  PluginMessage,
  PluginMessageBody,
  PluginNotificationSpec,
  PluginOpenResult,
  PluginOutgoingMessage,
  PluginPreview,
  PluginUiNode,
  ThreadId,
} from "./generated";

/** Codes of {@link PluginApiError}, the same as the codes of Nail's API errors. */
export type PluginApiErrorCode =
  | "forbidden"
  | "not_found"
  | "invalid_request"
  | "too_large"
  | "conflict"
  | "unsupported"
  | "rate_limited"
  | "network"
  | "unavailable"
  | "cancelled"
  | "internal"
  | (string & {});

/**
 * How a rejected API call looks: an `Error` (name `NailError`) with a
 * `code`. `forbidden` means the plug-in lacks the permission (or the grant
 * does not cover the account); `too_large` that storage is over its quota;
 * `rate_limited` that the plug-in sent too many notifications or timers.
 */
export interface PluginApiError extends Error {
  /** The error code. */
  code: PluginApiErrorCode;
}

/** Options of {@link NailMailApi.search}. */
export interface PluginMailSearchOptions {
  /** At most this many messages (the host clamps it). */
  limit?: number;
}

/**
 * The kind of a write, as Nail's confirmations and "always allow"
 * policies name it (`nail_api::agents::ActionKind`). Trash, permanent
 * delete and deleting events / contacts / tasks (`pim-delete`) always ask
 * the user; sending, invitations and rule changes ask unless the user
 * allows them for the plug-in; the other kinds run without asking unless
 * the user makes them ask.
 */
export type PluginActionKind =
  | "organize"
  | "trash"
  | "permanent-delete"
  | "draft"
  | "send"
  | "calendar"
  | "invite"
  | "contact"
  | "task"
  | "pim-delete"
  | "rule";

/**
 * How a write ended (the same answer MCP's write tools give, in
 * camelCase). Writes that need the user's confirmation resolve only after
 * the user decided (or after the confirmation expired, 5 minutes by
 * default). A denied or expired write is not an error: nothing changed.
 */
export type PluginWriteResult =
  | {
      /** Done: queued in the operation queue (undoable). */
      status: "done";
      /** The queued operations. */
      opIds: string[];
      /** Ids of what was created (draft, event, contact, task). */
      created: string[];
      /** The confirmation the user approved, if one was needed. */
      confirmationId?: string;
    }
  | {
      /** The user denied it; nothing changed. */
      status: "denied";
      /** The confirmation. */
      confirmationId: string;
    }
  | {
      /** Nobody decided in time; nothing changed. */
      status: "expired";
      /** The confirmation. */
      confirmationId: string;
    }
  | {
      /** A plug-in's before-send check stopped `nail.mail.send`; the draft is kept. */
      status: "vetoed";
      /** The reasons (`<plug-in name>: <reason>`). */
      reasons: string[];
    };

/** Changes of {@link NailMailApi.update}; fields left out stay as they are. */
export interface PluginMessageChanges {
  /** Mark read (`true`) or unread (`false`). */
  read?: boolean;
  /** Flag or unflag. */
  flagged?: boolean;
  /** Tags to add. */
  addTags?: string[];
  /** Tags to remove. */
  removeTags?: string[];
  /** Move to this folder of the same account. */
  moveToFolderId?: FolderId;
  /** Move to the account's Archive folder. */
  archive?: boolean;
  /** Snooze until this time (RFC 3339). */
  snoozeUntil?: string;
}

/** Options of {@link NailMailApi.delete}. */
export interface PluginDeleteOptions {
  /**
   * Delete for good instead of moving to Trash. The user always confirms
   * deletions; permanent ones are never confirmed automatically.
   */
  permanent?: boolean;
}

/** The format of a body or inserted text. */
export type PluginTextFormat = ComposerTextFormat;

/** A new draft ({@link NailMailApi.createDraft}). */
export interface PluginDraftInput {
  /** The sending identity (default: the account's default identity). */
  identityId?: IdentityId;
  /** Recipients (`Name <address>` or `address`). */
  to?: string[];
  /** Cc recipients. */
  cc?: string[];
  /** Bcc recipients. */
  bcc?: string[];
  /** Subject (default when replying: `Re: <subject>`). */
  subject?: string;
  /** The body. */
  body?: string;
  /** Format of `body` (default `markdown`). */
  format?: PluginTextFormat;
  /** Reply to this message (threading headers, quoted original). */
  replyToMessageId?: MessageId;
  /** With `replyToMessageId`: reply to all. */
  replyAll?: boolean;
  /** Forward this message inline. */
  forwardMessageId?: MessageId;
  /** Attachments (at most 20 per call). */
  attachments?: PluginDraftAttachment[];
}

/**
 * An attachment of a draft: its bytes (`data`, or `dataBase64`), or an
 * attachment of an existing message (copying it needs `mail.read.body`).
 */
export interface PluginDraftAttachment {
  /** File name. */
  filename: string;
  /** MIME type (default: from the file name). */
  mimeType?: string;
  /** The content (at most 25 MB). */
  data?: Uint8Array | ArrayBuffer;
  /** Or the content as base64. */
  dataBase64?: string;
  /** Or: copy an attachment of this message … */
  fromMessageId?: MessageId;
  /** … with this index. */
  fromAttachmentIndex?: number;
}

/** Changes of {@link NailMailApi.updateDraft}; fields left out stay as they are. */
export interface PluginDraftChanges {
  /** New recipients. */
  to?: string[];
  /** New Cc recipients. */
  cc?: string[];
  /** New Bcc recipients. */
  bcc?: string[];
  /** New subject. */
  subject?: string;
  /** New body. */
  body?: string;
  /** Format of `body` (default: the draft's). */
  format?: PluginTextFormat;
  /** Attachments to add. */
  addAttachments?: PluginDraftAttachment[];
  /** Positions of attachments to remove. */
  removeAttachments?: number[];
}

/** Options of {@link NailMailApi.send}. */
export interface PluginSendOptions {
  /** Send later (RFC 3339); default: now (after the user's undo-send delay). */
  sendAt?: string;
}

/**
 * `nail.mail`: mail. Headers need `mail.read`, bodies `mail.read.body`,
 * changes `mail.organize`, drafts and sending `mail.send` — each for the
 * message's (or the sending identity's) account.
 */
export interface NailMailApi {
  /** One message's headers. */
  get(id: MessageId): Promise<PluginMessage>;
  /**
   * One message's body. Always rejects with `forbidden` for end-to-end
   * encrypted mail.
   */
  getBody(id: MessageId): Promise<PluginMessageBody>;
  /**
   * Messages matching a query of Nail's search language
   * (`from:ada has:attachment`), newest first, in the granted accounts.
   * Message text is searched only where `mail.read.body` is granted:
   * elsewhere free text matches sender, recipients and subject, and a
   * query with `body:` or `filename:` rejects with `forbidden` when no
   * account allows it. End-to-end encrypted mail is left out while Nail
   * indexes its decrypted text.
   */
  search(query: string, options?: PluginMailSearchOptions): Promise<PluginMessage[]>;
  /** The messages of a thread, oldest first. */
  thread(threadId: ThreadId): Promise<PluginMessage[]>;
  /**
   * Flags, tags, read state, moves, archiving, snoozing (at most 500
   * messages). Needs `mail.organize` (action kind `organize`).
   */
  update(
    messageIds: MessageId | MessageId[],
    changes: PluginMessageChanges,
  ): Promise<PluginWriteResult>;
  /**
   * Moves messages to Trash, or deletes them for good. Needs
   * `mail.organize`; always asks the user (`trash` / `permanent-delete`).
   */
  delete(
    messageIds: MessageId | MessageId[],
    options?: PluginDeleteOptions,
  ): Promise<PluginWriteResult>;
  /**
   * Saves a new draft (never sends it); `created[0]` is its id. Needs
   * `mail.send` (`draft`).
   */
  createDraft(draft: PluginDraftInput): Promise<PluginWriteResult>;
  /** Changes a saved draft. Needs `mail.send` (`draft`). */
  updateDraft(draftId: DraftId, changes: PluginDraftChanges): Promise<PluginWriteResult>;
  /**
   * Sends a saved draft. Needs `mail.send`; asks the user unless they
   * allow this plug-in to send (`send`). Other plug-ins' before-send
   * hooks run as for the user's own mail.
   */
  send(draftId: DraftId, options?: PluginSendOptions): Promise<PluginWriteResult>;
}

/** Options of {@link NailCalendarApi.events}. */
export interface PluginCalendarEventsOptions {
  /** At most this many events (the host clamps it to 500). */
  limit?: number;
}

/** A calendar or task list a plug-in may write to. */
export interface PluginCollection {
  /** Its id. */
  id: CalendarId;
  /** Its name. */
  name: string;
  /** Whether it is the default one of its kind (for new items). */
  default: boolean;
  /** Read-only (a subscription): writes to it fail. */
  readOnly: boolean;
}

/** Which occurrences of a recurring event a change applies to. */
export type PluginEditScope = "this" | "this-and-following" | "thisAndFollowing" | "all";

/** A new event ({@link NailCalendarApi.createEvent}). */
export interface PluginEventInput {
  /** The calendar (default: the default calendar). */
  calendarId?: CalendarId;
  /** Title. */
  title: string;
  /** Start: RFC 3339, or `YYYY-MM-DD` for all-day events. */
  start: string;
  /** End (exclusive): RFC 3339, or `YYYY-MM-DD` for all-day events. */
  end: string;
  /** IANA zone of the times (default: the system's). */
  tz?: string;
  /** Location. */
  location?: string;
  /** Description (plain text). */
  description?: string;
  /** Attendee addresses. */
  attendees?: string[];
  /** An iCalendar `RRULE` value (`FREQ=WEEKLY;BYDAY=MO`). */
  recurrence?: string;
  /** Mail invitations to the attendees (also needs `mail.send`; action kind `invite`). */
  sendInvitations?: boolean;
}

/** Changes of {@link NailCalendarApi.updateEvent}; fields left out stay. */
export interface PluginEventChanges {
  /** For recurring events: the occurrence's original start (RFC 3339). */
  occurrence?: string;
  /** Which occurrences (default `all`). */
  scope?: PluginEditScope;
  /** New title. */
  title?: string;
  /** New start. */
  start?: string;
  /** New end. */
  end?: string;
  /** New zone. */
  tz?: string;
  /** New location. */
  location?: string;
  /** New description. */
  description?: string;
  /** Attendees to add. */
  addAttendees?: string[];
  /** Attendees to remove. */
  removeAttendees?: string[];
  /** Mail the change to the attendees (also needs `mail.send`). */
  sendUpdates?: boolean;
}

/** Options of {@link NailCalendarApi.deleteEvent}. */
export interface PluginEventDeleteOptions {
  /** For recurring events: the occurrence's original start. */
  occurrence?: string;
  /** Which occurrences (default `all`). */
  scope?: PluginEditScope;
}

/** An answer to an invitation ({@link NailCalendarApi.respond}). */
export interface PluginInviteAnswer {
  /** The invitation message … */
  messageId?: MessageId;
  /** … or the event (exactly one of the two). */
  eventId?: EventId;
  /** For recurring events: the occurrence (original start); default: the whole series. */
  occurrence?: string;
  /** The answer. */
  response: "accept" | "tentative" | "decline";
  /** A note to the organizer. */
  comment?: string;
  /** Tell the organizer (default `true`; needs `mail.send`, action kind `invite`). */
  send?: boolean;
}

/**
 * `nail.calendar`: the user's calendars. Reading needs `calendar.read`,
 * writing `calendar.write`.
 */
export interface NailCalendarApi {
  /**
   * The event occurrences overlapping `[from, to)` (RFC 3339 times, at
   * most 92 days apart) in every calendar the user shows, by start.
   * Recurring events come as one entry per occurrence.
   */
  events(
    from: string,
    to: string,
    options?: PluginCalendarEventsOptions,
  ): Promise<PluginCalendarEvent[]>;
  /** The calendars the plug-in may write to. Needs `calendar.write`. */
  calendars(): Promise<PluginCollection[]>;
  /** Creates an event. Needs `calendar.write` (`calendar`, or `invite` with invitations). */
  createEvent(event: PluginEventInput): Promise<PluginWriteResult>;
  /** Changes an event. Needs `calendar.write`. */
  updateEvent(eventId: EventId, changes: PluginEventChanges): Promise<PluginWriteResult>;
  /** Deletes an event. Needs `calendar.write`; always asks the user (`pim-delete`). */
  deleteEvent(eventId: EventId, options?: PluginEventDeleteOptions): Promise<PluginWriteResult>;
  /**
   * Answers an invitation. Needs `calendar.write` (and `mail.send` to tell
   * the organizer; `invite`, else `calendar`).
   */
  respond(answer: PluginInviteAnswer): Promise<PluginWriteResult>;
}

/** A contact as {@link NailContactsApi.find} returns it. */
export interface PluginContact {
  /** Its id. */
  id: string;
  /** Its address book. */
  addressBookId: string;
  /** Full name. */
  fullName?: string;
  /** Email addresses. */
  emails: string[];
  /** Organization. */
  organization?: string;
}

/** Options of {@link NailContactsApi.find}. */
export interface PluginContactFindOptions {
  /** At most this many contacts (the host clamps it). */
  limit?: number;
}

/** A new contact or changes of one ({@link NailContactsApi.upsert}). */
export interface PluginContactInput {
  /** The contact to change; absent = create one. */
  contactId?: string;
  /** For new contacts: the address book (default: the default one). */
  addressBookId?: string;
  /** Full name. */
  fullName?: string;
  /** Email addresses (replace the existing ones). */
  emails?: string[];
  /** Phone numbers (replace the existing ones). */
  phones?: string[];
  /** Organization. */
  organization?: string;
  /** Job title. */
  title?: string;
  /** Notes. */
  notes?: string;
}

/** `nail.contacts`: contact writes. Needs `contacts.write`. */
export interface NailContactsApi {
  /** The address books the plug-in may write to. */
  addressBooks(): Promise<PluginCollection[]>;
  /** The contacts with this email address (case-insensitive). */
  find(email: string, options?: PluginContactFindOptions): Promise<PluginContact[]>;
  /** Creates or changes a contact (`contact`). */
  upsert(contact: PluginContactInput): Promise<PluginWriteResult>;
  /** Deletes a contact; always asks the user (`pim-delete`). */
  delete(contactId: string): Promise<PluginWriteResult>;
}

/** A new task ({@link NailTasksApi.create}). */
export interface PluginTaskInput {
  /** The task list (default: the default one). */
  list?: CalendarId;
  /** The same as `list` (pass one of them). */
  listId?: CalendarId;
  /** Title. */
  title: string;
  /** Due: RFC 3339 or `YYYY-MM-DD`. */
  due?: string;
  /** Notes (plain text). */
  notes?: string;
  /** Priority 1 (highest) … 9 (lowest); 0 = none. */
  priority?: number;
  /** Created as done. */
  completed?: boolean;
  /**
   * A link to the message the task is about (`nail://messages/<id>`, a
   * message's `uri`; at most one message). Needs `mail.read` for it.
   */
  links?: string[];
  /** The same as a `links` entry, as a message id. */
  relatedMessageId?: MessageId;
}

/** Changes of {@link NailTasksApi.update}; fields left out stay as they are. */
export interface PluginTaskChanges {
  /** New title. */
  title?: string;
  /** New due date; `""` removes it. */
  due?: string;
  /** New notes. */
  notes?: string;
  /** New priority. */
  priority?: number;
  /** Done or open. */
  completed?: boolean;
}

/** `nail.tasks`: task writes. Needs `tasks.write`. */
export interface NailTasksApi {
  /** The task lists the plug-in may write to. */
  lists(): Promise<PluginCollection[]>;
  /** Creates a task (`task`); `created[0]` is its id. */
  create(task: PluginTaskInput): Promise<PluginWriteResult>;
  /** Changes a task (`task`). */
  update(taskId: string, changes: PluginTaskChanges): Promise<PluginWriteResult>;
  /** Deletes a task; always asks the user (`pim-delete`). */
  delete(taskId: string): Promise<PluginWriteResult>;
}

/** Options of the composer edits. */
export interface PluginComposerEditOptions {
  /**
   * The draft (default: the draft of the latest composer action of this
   * plug-in in the UI that ran it).
   */
  draftId?: DraftId;
  /** The format of the text (default `text`; HTML is sanitized). */
  format?: PluginTextFormat;
}

/**
 * `nail.composer`: composer actions, Markdown blocks and edits of the
 * open draft, applied live (the user can undo them like typing). Needs
 * `composer`. Edits work only on a draft a composer action of this
 * plug-in ran on, for a short while after it ran.
 */
export interface NailComposerApi extends PluginComposerRegistry {
  /** Inserts text at the cursor. */
  insert(text: string, options?: PluginComposerEditOptions): Promise<void>;
  /** Replaces the selection (inserts at the cursor without one). */
  replaceSelection(text: string, options?: PluginComposerEditOptions): Promise<void>;
  /** Replaces the whole body. */
  replaceAll(text: string, options?: PluginComposerEditOptions): Promise<void>;
}

/** Arguments of a localized message (`{ $name }` in the catalog). */
export type PluginL10nArgs = Record<string, string | number | boolean | null | undefined>;

/**
 * `nail.l10n`: the plug-in's own Fluent catalogs (`contributes.locales`,
 * the same ones `%id%` manifest texts use). Synchronous; no permission
 * needed.
 */
export interface NailL10nApi {
  /** The language Nail shows (`en`, `de`, `pt-BR`). */
  readonly locale: string;
  /**
   * Message `id` in that language, else in English, else the id itself
   * (per message, like Nail's own texts).
   */
  t(id: string, args?: PluginL10nArgs): string;
  /** Whether a catalog of the language or the English one has message `id`. */
  has(id: string): boolean;
}

/** A click on a `button` node, with the current values of the tree's inputs. */
export interface PluginUiAction {
  /** The button's `id`. */
  componentId: string;
  /** The values of the input nodes by id. */
  values: Record<string, JsonValue>;
}

/** Called for a `button` node inside a dialog body or panel. */
export type PluginUiActionHandler = (action: PluginUiAction) => void | Promise<void>;

/** Options of {@link NailUiApi.showDialog}. */
export interface PluginDialogOptions {
  /** Called when a `button` node in the body is clicked (the dialog stays open). */
  onAction?: PluginUiActionHandler;
}

/** Runs a command; `ctx` says where it was invoked and what is selected. */
export type PluginCommandHandler = (ctx: PluginCommandContext) => void | Promise<void>;

/** Draws a panel for the UI's current context. */
export type PluginPanelRender = (
  ctx: PluginCommandContext,
) => PluginUiNode[] | Promise<PluginUiNode[]>;

/** What draws a panel declared in the manifest (`contributes.panels`). */
export interface PluginPanelProvider {
  /** Returns the panel's component tree. */
  render: PluginPanelRender;
  /** Called for a `button` node in the panel. */
  onAction?: PluginUiActionHandler;
}

/**
 * `nail.ui`: commands, panels, dialogs (`ui.dialogs`) and notifications
 * (`notifications`).
 */
export interface NailUiApi {
  /** Registers the handler of a command of the manifest (same as `nail.commands.register`). */
  registerCommand(name: string, handler: PluginCommandHandler): void;
  /** Registers the provider (or render function) of a panel of the manifest. */
  registerPanel(name: string, panel: PluginPanelProvider | PluginPanelRender): void;
  /**
   * Shows a dialog in the UI that ran the command and resolves when it
   * closes. Needs `ui.dialogs`.
   */
  showDialog(spec: PluginDialogSpec, options?: PluginDialogOptions): Promise<PluginDialogResult>;
  /**
   * Shows a notification; a plain string is its title (1–200 characters;
   * body at most 2000). Needs `notifications`; too many in a minute
   * reject with `rate_limited`.
   */
  notify(notification: PluginNotificationSpec | string): Promise<void>;
  /** Replaces the content of an open panel (for example after data changed). */
  updatePanel(name: string, tree: PluginUiNode[]): Promise<void>;
}

/** `nail.commands`: the handlers of the manifest's commands. No permission needed. */
export interface NailCommandsApi {
  /** Registers the handler of a command declared in `contributes.commands`. */
  register(name: string, handler: PluginCommandHandler): void;
}

/** Space used by {@link NailStorageApi}. */
export interface PluginStorageUsage {
  /** Stored keys. */
  keys: number;
  /** Bytes used (keys plus JSON values). */
  usedBytes: number;
  /** The quota in bytes (5 MiB unless the user changed it). */
  quotaBytes: number;
}

/**
 * `nail.storage`: the plug-in's own key-value storage on this device
 * (`storage`). Values are JSON; keys at most 256 bytes, values 1 MiB.
 * Writing past the quota rejects with `too_large`.
 */
export interface NailStorageApi {
  /** The value of a key, or `null` when it is not set. */
  get<T extends JsonValue = JsonValue>(key: string): Promise<T | null>;
  /** Sets a key. */
  set(key: string, value: JsonValue): Promise<void>;
  /** Removes a key; whether it existed. */
  delete(key: string): Promise<boolean>;
  /** The keys, sorted; only those starting with `prefix` when given. */
  keys(prefix?: string): Promise<string[]>;
  /** Removes every key. */
  clear(): Promise<void>;
  /** How much of the quota is used. */
  usage(): Promise<PluginStorageUsage>;
}

/** Methods of {@link NailHttpApi.fetch}. */
export type PluginHttpMethod = "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE";

/** A request of {@link NailHttpApi.fetch}. */
export interface PluginHttpRequest {
  /** `GET` unless given. */
  method?: PluginHttpMethod;
  /**
   * Request headers. `Host`, `Cookie`, `Content-Length`, hop-by-hop and
   * `Proxy-*` headers are refused.
   */
  headers?: Record<string, string>;
  /**
   * The body: text (serialize JSON yourself) or bytes (`Uint8Array`,
   * `ArrayBuffer`); not for `GET` / `HEAD`.
   */
  body?: string | Uint8Array | ArrayBuffer;
}

/** The response of {@link NailHttpApi.fetch} (the body is read completely). */
export interface PluginHttpResponse {
  /** The HTTP status. */
  readonly status: number;
  /** Whether the status is 200–299. */
  readonly ok: boolean;
  /** Response headers, names in lower case. */
  readonly headers: Record<string, string>;
  /** The body as text (UTF-8; malformed bytes become U+FFFD). */
  readonly body: string;
  /** The body as text. */
  text(): Promise<string>;
  /** The body parsed as JSON. */
  json<T = JsonValue>(): Promise<T>;
  /** The body's bytes (images, PDFs, archives). */
  bytes(): Promise<Uint8Array>;
  /** The body's bytes as an `ArrayBuffer`. */
  arrayBuffer(): Promise<ArrayBuffer>;
}

/**
 * `nail.http`: requests to the hosts of the manifest's `network:<host>`
 * permissions, made by Nail's core on the plug-in's behalf: `https:` on
 * the default port (`http:` and ports only for loopback hosts), no
 * credentials in the URL, redirects only to allowed hosts. Any other host
 * rejects with `forbidden`, a malformed request with `invalid_request`.
 */
export interface NailHttpApi {
  /** Fetches `url`. */
  fetch(url: string, request?: PluginHttpRequest): Promise<PluginHttpResponse>;
}

/**
 * `nail.settings`: the values of the manifest's settings schema, as the
 * user set them on the plug-in's generated settings page (defaults
 * otherwise). Synchronous; no permission needed.
 */
export interface NailSettingsApi {
  /** One setting's value (`undefined` for a key the schema does not have). */
  get<T extends JsonValue = JsonValue>(key: string): T | undefined;
  /** Every setting's value. */
  all(): Record<string, JsonValue>;
  /** Called with every value whenever the user changes a setting. */
  onChange(listener: (values: Record<string, JsonValue>) => void): void;
}

/**
 * `nail.log`: lines in the plug-in's log (plug-in manager, `nail plugin
 * dev`). `console.*` writes there too. No permission needed.
 */
export interface NailLogApi {
  /** A debug line. */
  debug(...args: unknown[]): void;
  /** An information line. */
  info(...args: unknown[]): void;
  /** A warning. */
  warn(...args: unknown[]): void;
  /** An error. */
  error(...args: unknown[]): void;
}

/** The plug-in's identity. */
export interface PluginIdentity {
  /** Its manifest's `id`. */
  readonly id: PluginId;
  /** Its manifest's `version`. */
  readonly version: string;
}

/** The global `nail` object the host installs. */
export interface NailApi {
  /** The plug-in's id and version. */
  readonly plugin: PluginIdentity;
  /** The API version of the host (`"1"`). */
  readonly apiVersion: string;
  /** Mail: reading, organizing, drafts and sending. */
  readonly mail: NailMailApi;
  /** Calendars. */
  readonly calendar: NailCalendarApi;
  /** Contacts (writes). */
  readonly contacts: NailContactsApi;
  /** Tasks (writes). */
  readonly tasks: NailTasksApi;
  /** Composer actions, Markdown blocks and edits of the open draft. */
  readonly composer: NailComposerApi;
  /** Mail hooks (also `ctx.hooks`). */
  readonly hooks: PluginHooksRegistry;
  /** Attachment previews and actions (also `ctx.attachments`). */
  readonly attachments: PluginAttachmentsRegistry;
  /** Link actions (also `ctx.links`). */
  readonly links: PluginLinksRegistry;
  /** Background jobs (also `ctx.jobs`). */
  readonly jobs: PluginJobsRegistry;
  /** MCP tools (also `ctx.mcp`). */
  readonly mcp: PluginMcpRegistry;
  /** Commands, panels, dialogs and notifications. */
  readonly ui: NailUiApi;
  /** Command handlers. */
  readonly commands: NailCommandsApi;
  /** Key-value storage. */
  readonly storage: NailStorageApi;
  /** HTTP to declared hosts. */
  readonly http: NailHttpApi;
  /** The plug-in's settings. */
  readonly settings: NailSettingsApi;
  /** The plug-in's own translations. */
  readonly l10n: NailL10nApi;
  /** The plug-in's log. */
  readonly log: NailLogApi;
}

/**
 * A message as an arrival or open hook gets it: the headers, plus the
 * body when the plug-in has `mail.read.body` for the account (never for
 * end-to-end encrypted mail).
 */
export interface PluginHookMessage {
  /** The message's headers. */
  message: PluginMessage;
  /** Its body, with `mail.read.body`. */
  body?: PluginMessageBody;
}

/**
 * The arrival hook (`hooks.arrival`): called for each new message that
 * matches the hook's `filter`, after the user's own rules. Answers actions
 * (they need `mail.organize`), or nothing. Budget: 2 s by default.
 */
export type PluginArrivalHook = (
  mail: PluginHookMessage,
) =>
  | PluginArrivalResult
  | null
  | undefined
  | void
  | Promise<PluginArrivalResult | null | undefined | void>;

/**
 * The before-send hook (`hooks.beforeSend`): called with every outgoing
 * message before it is queued. Answers headers / content to add or a veto,
 * or nothing. Budget: 3 s by default. A hook that throws or runs out of
 * time is skipped: it never blocks sending.
 */
export type PluginBeforeSendHook = (
  message: PluginOutgoingMessage,
) =>
  | PluginBeforeSendResult
  | null
  | undefined
  | void
  | Promise<PluginBeforeSendResult | null | undefined | void>;

/**
 * The open hook (`hooks.open`): called when the user opens a message that
 * matches the hook's `filter`. Answers banners and badges, or nothing.
 * Budget: 1 s by default.
 */
export type PluginOpenHook = (
  mail: PluginHookMessage,
) =>
  PluginOpenResult | null | undefined | void | Promise<PluginOpenResult | null | undefined | void>;

/** A press on a button of a banner an open hook answered. */
export interface PluginBannerAction {
  /** The message the banner is shown with. */
  messageId: MessageId;
  /** The banner's `id`. */
  bannerId: string;
  /** The button's `id`. */
  buttonId: string;
}

/** Called when the user presses a banner button. */
export type PluginBannerActionHandler = (action: PluginBannerAction) => void | Promise<void>;

/** Handlers of the hook kinds. */
export interface PluginHookHandlers {
  /** `arrival`. */
  arrival: PluginArrivalHook;
  /** `beforeSend`. */
  beforeSend: PluginBeforeSendHook;
  /** `open`. */
  open: PluginOpenHook;
}

/**
 * A composer, attachment or link action. `ctx.composer`, `ctx.attachment`
 * or `ctx.link` says what it was invoked on; attachment actions also get
 * the attachment's bytes.
 */
export type PluginActionHandler = (
  ctx: PluginActionContext,
  bytes?: Uint8Array,
) => void | Promise<void>;

/** Renders a Markdown block (```` ```<plugin>.<block> ````) to HTML and text. */
export type PluginMarkdownBlockRenderer = (
  input: PluginMarkdownBlockInput,
) => PluginMarkdownBlockResult | Promise<PluginMarkdownBlockResult>;

/** Renders an attachment preview: a component tree or a raster image, never HTML. */
export type PluginPreviewRenderer = (
  attachment: PluginAttachmentRef,
  bytes: Uint8Array,
) => PluginPreview | Promise<PluginPreview>;

/** Runs a background job; the trigger says why. */
export type PluginJobHandler = (trigger: PluginJobTrigger) => void | Promise<void>;

/**
 * Serves an MCP tool call (`contributes.mcpTools`): the agent's arguments
 * in, a JSON result (or text) out. The result is handed to the agent as
 * untrusted data.
 */
export type PluginMcpToolHandler = (
  args: Record<string, JsonValue>,
) => JsonValue | undefined | void | Promise<JsonValue | undefined | void>;

/** Hook registrations (`ctx.hooks`). */
export interface PluginHooksRegistry {
  /** Registers the handler of a hook of the manifest (`contributes.hooks`). */
  register<K extends PluginHookKind>(hook: K, handler: PluginHookHandlers[K]): void;
  /** Registers the handler of banner button presses. */
  onBannerAction(handler: PluginBannerActionHandler): void;
}

/** Composer registrations (`ctx.composer`). */
export interface PluginComposerRegistry {
  /** Registers the handler of a composer action (`contributes.composerActions`). */
  registerAction(name: string, handler: PluginActionHandler): void;
  /** Registers the renderer of a Markdown block (`contributes.markdownBlocks`). */
  registerBlock(name: string, renderer: PluginMarkdownBlockRenderer): void;
}

/** Attachment registrations (`ctx.attachments`). */
export interface PluginAttachmentsRegistry {
  /** Registers a preview renderer (`contributes.attachmentPreviews`). */
  registerPreview(name: string, renderer: PluginPreviewRenderer): void;
  /** Registers an attachment action (`contributes.attachmentActions`). */
  registerAction(name: string, handler: PluginActionHandler): void;
}

/** Link registrations (`ctx.links`). */
export interface PluginLinksRegistry {
  /** Registers a link action (`contributes.linkActions`). */
  registerAction(name: string, handler: PluginActionHandler): void;
}

/** Background job registrations (`ctx.jobs`). */
export interface PluginJobsRegistry {
  /** Registers the handler of a job (`contributes.jobs`). */
  register(id: string, handler: PluginJobHandler): void;
}

/** MCP tool registrations (`ctx.mcp`). */
export interface PluginMcpRegistry {
  /** Registers the handler of a tool (`contributes.mcpTools`). */
  registerTool(id: string, handler: PluginMcpToolHandler): void;
}

/** What the host passes to `activate` (`ctx` in the examples). */
export interface PluginActivationContext {
  /** The plug-in's id and version. */
  readonly plugin: PluginIdentity;
  /** Command handlers. */
  readonly commands: NailCommandsApi;
  /** Panel providers. */
  readonly panels: {
    /** Registers the provider (or render function) of a panel of the manifest. */
    register(name: string, panel: PluginPanelProvider | PluginPanelRender): void;
  };
  /** The plug-in's settings. */
  readonly settings: NailSettingsApi;
  /** Mail hooks. */
  readonly hooks: PluginHooksRegistry;
  /** Composer actions and Markdown blocks. */
  readonly composer: PluginComposerRegistry;
  /** Attachment previews and actions. */
  readonly attachments: PluginAttachmentsRegistry;
  /** Link actions. */
  readonly links: PluginLinksRegistry;
  /** Background jobs. */
  readonly jobs: PluginJobsRegistry;
  /** MCP tools. */
  readonly mcp: PluginMcpRegistry;
  /** The plug-in's own translations. */
  readonly l10n: NailL10nApi;
}

/**
 * A plug-in: the default export of its bundle (see `definePlugin`). A
 * default export that is a function is taken as `activate`.
 */
export interface PluginDefinition {
  /** Called once when the plug-in starts: register commands and panels here. */
  activate(ctx: PluginActivationContext): void | Promise<void>;
  /**
   * Called when the plug-in stops (disabled, reloaded, Nail quits). It
   * runs synchronously: the host does not wait for a returned promise.
   */
  deactivate?(): void;
}
