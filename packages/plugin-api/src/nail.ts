/**
 * `nail` — the SDK's typed view of the host's global `nail` object, with
 * a few conveniences: dialog bodies and panels may be JSX (one node or a
 * list), and a missing host (code run outside Nail) fails with a clear
 * error instead of `undefined`.
 */
import type {
  NailApi,
  NailCalendarApi,
  NailCommandsApi,
  NailComposerApi,
  NailContactsApi,
  NailHttpApi,
  NailL10nApi,
  NailLogApi,
  NailMailApi,
  NailSettingsApi,
  NailStorageApi,
  NailTasksApi,
  PluginApiError,
  PluginApiErrorCode,
  PluginCommandHandler,
  PluginDialogOptions,
  PluginIdentity,
  PluginPanelProvider,
  PluginUiActionHandler,
} from "./api";
import type {
  PluginCommandContext,
  PluginDialogResult,
  PluginDialogSpec,
  PluginNotificationSpec,
} from "./generated";
import { toNodes, type UiChild } from "./jsx-runtime";

declare global {
  /**
   * The host API, installed by Nail before the plug-in's bundle runs.
   * The SDK's `nail` export wraps it and adds JSX conveniences.
   */
  var nail: NailApi;
}

/** A dialog spec whose body may be JSX. */
export type DialogSpec = Omit<PluginDialogSpec, "body"> & {
  /** The content: nodes, JSX, strings (text nodes), or a list of them. */
  body: UiChild;
};

/** Draws a panel; may return JSX. */
export type PanelRender = (ctx: PluginCommandContext) => UiChild | Promise<UiChild>;

/** A panel provider whose `render` may return JSX. */
export interface PanelProvider {
  /** Returns the panel's content for the UI's current context. */
  render: PanelRender;
  /** Called for a `button` node in the panel. */
  onAction?: PluginUiActionHandler;
}

/** The SDK's `nail.ui`. */
export interface SdkUiApi {
  /** Registers the handler of a command of the manifest. */
  registerCommand(name: string, handler: PluginCommandHandler): void;
  /** Registers the provider (or render function) of a panel of the manifest. */
  registerPanel(name: string, panel: PanelProvider | PanelRender): void;
  /** Shows a dialog; resolves with the button and values when it closes. Needs `ui.dialogs`. */
  showDialog(spec: DialogSpec, options?: PluginDialogOptions): Promise<PluginDialogResult>;
  /** Shows a notification; a string is its title. Needs `notifications`. */
  notify(notification: PluginNotificationSpec | string): Promise<void>;
  /** Replaces the content of an open panel. */
  updatePanel(name: string, content: UiChild): Promise<void>;
}

/** The SDK's `nail` object (see {@link NailApi} for the host side). */
export interface SdkNail {
  /** The plug-in's id and version. */
  readonly plugin: PluginIdentity;
  /** The API version of the host. */
  readonly apiVersion: string;
  /** Mail: reading, organizing, drafts and sending. */
  readonly mail: NailMailApi;
  /** Calendars: reading (`calendar.read`) and writing (`calendar.write`). */
  readonly calendar: NailCalendarApi;
  /** Contacts (`contacts.write`). */
  readonly contacts: NailContactsApi;
  /** Tasks (`tasks.write`). */
  readonly tasks: NailTasksApi;
  /** Edits of the open draft (`composer`). */
  readonly composer: NailComposerApi;
  /** Commands, panels, dialogs and notifications. */
  readonly ui: SdkUiApi;
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

/** Builds an error shaped like the host's API errors. */
export function createPluginApiError(code: PluginApiErrorCode, message: string): PluginApiError {
  const error = new Error(message) as PluginApiError;
  error.name = "NailError";
  error.code = code;
  return error;
}

/** The host's API; throws when the code does not run inside Nail. */
export function host(): NailApi {
  const api = (globalThis as { nail?: NailApi | null }).nail;
  if (api === undefined || api === null) {
    throw createPluginApiError(
      "unsupported",
      "The Nail plug-in API is not available: this code must run inside Nail",
    );
  }
  return api;
}

/**
 * Whether `error` is an API error (optionally with this `code`), e.g.
 * `isPluginApiError(e, "forbidden")` for a missing permission.
 */
export function isPluginApiError(
  error: unknown,
  code?: PluginApiErrorCode,
): error is PluginApiError {
  if (typeof error !== "object" || error === null) return false;
  const c = (error as { code?: unknown }).code;
  return typeof c === "string" && (code === undefined || c === code);
}

/** Normalizes a dialog spec (JSX body → list of nodes). */
export function toDialogSpec(spec: DialogSpec): PluginDialogSpec {
  return { ...spec, body: toNodes(spec.body) };
}

/** Normalizes a notification (string → `{title}`). */
export function toNotificationSpec(
  notification: PluginNotificationSpec | string,
): PluginNotificationSpec {
  return typeof notification === "string" ? { title: notification } : notification;
}

/** Normalizes a panel (render function or provider; JSX → list of nodes). */
export function toPanelProvider(panel: PanelProvider | PanelRender): PluginPanelProvider {
  const provider: PanelProvider = typeof panel === "function" ? { render: panel } : panel;
  const out: PluginPanelProvider = {
    render: async (ctx) => toNodes(await provider.render(ctx)),
  };
  if (provider.onAction) out.onAction = provider.onAction;
  return out;
}

/** Forwards every property of `pick(host())`, looked up at call time. */
function forward<T extends object>(pick: (api: NailApi) => T): T {
  return new Proxy({} as T, {
    get(_target, prop) {
      const target = pick(host()) as Record<PropertyKey, unknown>;
      const value = target[prop];
      return typeof value === "function"
        ? (value as (...args: unknown[]) => unknown).bind(target)
        : value;
    },
  });
}

const ui: SdkUiApi = {
  registerCommand: (name, handler) => {
    host().ui.registerCommand(name, handler);
  },
  registerPanel: (name, panel) => {
    host().ui.registerPanel(name, toPanelProvider(panel));
  },
  showDialog: (spec, options) => host().ui.showDialog(toDialogSpec(spec), options),
  notify: (notification) => host().ui.notify(toNotificationSpec(notification)),
  updatePanel: (name, content) => host().ui.updatePanel(name, toNodes(content)),
};

/**
 * The plug-in API. Every call is checked against the plug-in's
 * permissions; see the permissions reference.
 */
export const nail: SdkNail = {
  get plugin() {
    return host().plugin;
  },
  get apiVersion() {
    return host().apiVersion;
  },
  mail: forward((api) => api.mail),
  calendar: forward((api) => api.calendar),
  contacts: forward((api) => api.contacts),
  tasks: forward((api) => api.tasks),
  composer: forward((api) => api.composer),
  ui,
  commands: forward((api) => api.commands),
  storage: forward((api) => api.storage),
  http: forward((api) => api.http),
  settings: forward((api) => api.settings),
  l10n: forward((api) => api.l10n),
  log: forward((api) => api.log),
};
