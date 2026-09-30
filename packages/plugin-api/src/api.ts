/**
 * The plug-in API of the preview (`apiVersion` "1"): what a plug-in bundle
 * sees at run time. The host (`crates/nail-plugins`, its `prelude.js`)
 * installs a frozen global `nail` object implementing {@link NailApi},
 * plus `console`, `setTimeout`, `clearTimeout` and `queueMicrotask`
 * (typed by `@nail/plugin-api/globals`), then loads the bundle and calls
 * its default export's `activate` ({@link PluginDefinition}).
 *
 * Calls that reach Nail return promises and pass the permission gate
 * first: a call without the matching permission rejects with a
 * {@link PluginApiError} whose `code` is `"forbidden"`. An error a command
 * handler, panel or `activate` does not catch stops the plug-in, so catch
 * the errors you expect.
 */
import type {
  JsonValue,
  MessageId,
  PluginCommandContext,
  PluginDialogResult,
  PluginDialogSpec,
  PluginId,
  PluginMessage,
  PluginMessageBody,
  PluginNotificationSpec,
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
 * `nail.mail`: read access to mail. Headers need `mail.read`, bodies
 * `mail.read.body`, each for the message's account.
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
  /** A text body (serialize JSON yourself); not for `GET` / `HEAD`. */
  body?: string;
}

/** The response of {@link NailHttpApi.fetch} (the body is read completely). */
export interface PluginHttpResponse {
  /** The HTTP status. */
  readonly status: number;
  /** Whether the status is 200–299. */
  readonly ok: boolean;
  /** Response headers, names in lower case. */
  readonly headers: Record<string, string>;
  /** The body as text. */
  readonly body: string;
  /** The body as text. */
  text(): Promise<string>;
  /** The body parsed as JSON. */
  json<T = JsonValue>(): Promise<T>;
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
  /** Mail (read only in the preview). */
  readonly mail: NailMailApi;
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
  /** The plug-in's log. */
  readonly log: NailLogApi;
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
