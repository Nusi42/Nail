/**
 * `@nail/plugin-api/testing` — an in-memory stand-in for Nail's plug-in
 * host, for unit tests of plug-ins (Vitest, `node:test`, …). It implements
 * the global `nail` object with the host's permission rules: every call
 * needs its permission and rejects with `forbidden` otherwise.
 *
 * ```ts
 * const host = createTestHost({ manifest, dialog: () => ({ button: "ok", values: {} }) });
 * const restore = host.install();
 * await plugin.activate(host.activationContext());
 * await host.runCommand("hello");
 * expect(host.dialogs).toHaveLength(1);
 * restore();
 * ```
 */
import type {
  NailApi,
  PluginActivationContext,
  PluginCommandHandler,
  PluginHttpRequest,
  PluginPanelProvider,
  PluginPanelRender,
  PluginStorageUsage,
} from "./api";
import type {
  JsonValue,
  PluginCommandContext,
  PluginDialogResult,
  PluginDialogSpec,
  PluginLogLevel,
  PluginManifest,
  PluginMessage,
  PluginMessageBody,
  PluginNotificationSpec,
  PluginPermission,
  PluginUiNode,
} from "./generated";
import { createPluginApiError } from "./nail";
import { checkTree } from "./tree";

/** Every permission except `network:*` (the default grant of a test host without a manifest). */
export const ALL_FIXED_PERMISSIONS: PluginPermission[] = [
  "mail.read",
  "mail.read.body",
  "calendar.read",
  "ui.dialogs",
  "notifications",
  "storage",
];

/** Answers a dialog in a test. */
export type DialogResponder = (
  spec: PluginDialogSpec,
) => PluginDialogResult | Promise<PluginDialogResult>;

/** A canned HTTP response. */
export interface TestHttpResponse {
  /** Status (default 200). */
  status?: number;
  /** Headers. */
  headers?: Record<string, string>;
  /** Body text. */
  body?: string;
}

/** Options of {@link createTestHost}. */
export interface TestHostOptions {
  /**
   * The plug-in's manifest: gives the id, version and (unless
   * `permissions` is set) the granted permissions; commands and panels
   * must then be declared in it to be registered.
   */
  manifest?: PluginManifest;
  /** The granted permissions (default: the manifest's, else {@link ALL_FIXED_PERMISSIONS}). */
  permissions?: PluginPermission[];
  /** Messages `nail.mail` can see. */
  messages?: PluginMessage[];
  /** Bodies by message id. */
  bodies?: Record<string, PluginMessageBody>;
  /** Setting values. */
  settings?: Record<string, JsonValue>;
  /** Answers dialogs (default: dismissed, `{button: null, values: {}}`). */
  dialog?: DialogResponder;
  /** Answers `nail.http.fetch` for allowed hosts (default 404). */
  fetch?: (url: string, request: PluginHttpRequest) => TestHttpResponse | Promise<TestHttpResponse>;
  /** Storage quota in bytes (default 5 MiB). */
  storageQuota?: number;
}

/** A log line recorded by the test host. */
export interface TestLogLine {
  /** Its level. */
  level: PluginLogLevel;
  /** The arguments joined as text. */
  message: string;
}

/** The test host (see {@link createTestHost}). */
export interface TestHost {
  /** The `nail` object. */
  readonly api: NailApi;
  /** Dialogs shown, in order. */
  readonly dialogs: PluginDialogSpec[];
  /** Notifications shown, in order. */
  readonly notifications: PluginNotificationSpec[];
  /** Panel updates (`nail.ui.updatePanel`), in order. */
  readonly panelUpdates: { name: string; tree: PluginUiNode[] }[];
  /** Log lines. */
  readonly logs: TestLogLine[];
  /** The storage contents. */
  readonly storage: Map<string, JsonValue>;
  /** Registered command names. */
  commandNames(): string[];
  /** Registered panel names. */
  panelNames(): string[];
  /** Installs `api` as `globalThis.nail`; returns a function restoring the previous value. */
  install(): () => void;
  /** The context the host passes to `activate`. */
  activationContext(): PluginActivationContext;
  /** Runs a registered command like a click in the UI would. */
  runCommand(name: string, ctx?: Partial<PluginCommandContext>): Promise<void>;
  /** Renders a registered panel (checking the tree). */
  renderPanel(name: string, ctx?: Partial<PluginCommandContext>): Promise<PluginUiNode[]>;
  /** Clicks a `button` node in a panel. */
  clickPanelButton(
    name: string,
    componentId: string,
    values?: Record<string, JsonValue>,
  ): Promise<void>;
  /** Changes settings and calls the `onChange` listeners. */
  changeSettings(values: Record<string, JsonValue>): void;
}

function commandContext(ctx: Partial<PluginCommandContext> = {}): PluginCommandContext {
  return { surface: "global", ...ctx, selection: ctx.selection ?? { messageIds: [] } };
}

function hostAllowed(pattern: string, host: string): boolean {
  if (pattern.startsWith("*.")) return host.endsWith(`.${pattern.slice(2)}`);
  return host === pattern;
}

/** Scheme and host of a URL (no `URL` global: plug-ins run without Node or DOM). */
function parseUrl(url: string): { protocol: string; hostname: string } {
  const match = /^([a-z][a-z0-9+.-]*:)\/\/(?:[^@/?#]*@)?(\[[^\]]*\]|[^:/?#]*)/i.exec(url);
  if (!match) throw createPluginApiError("invalid_request", `not a URL: ${url}`);
  return { protocol: (match[1] ?? "").toLowerCase(), hostname: (match[2] ?? "").toLowerCase() };
}

/** A JSON copy, as a value that went through Nail's storage would be. */
function clone(value: JsonValue): JsonValue {
  return JSON.parse(JSON.stringify(value)) as JsonValue;
}

function format(args: unknown[]): string {
  return args
    .map((a) => {
      if (typeof a === "string") return a;
      if (a instanceof Error) return `${a.name}: ${a.message}`;
      try {
        return JSON.stringify(a) ?? String(a);
      } catch {
        return String(a);
      }
    })
    .join(" ");
}

/** Creates an in-memory host. */
export function createTestHost(options: TestHostOptions = {}): TestHost {
  const manifest = options.manifest;
  const plugin = Object.freeze({
    id: manifest?.id ?? "test-plugin",
    version: manifest?.version ?? "0.0.0",
  });
  const granted = new Set<string>(
    options.permissions ?? manifest?.permissions ?? ALL_FIXED_PERMISSIONS,
  );
  const declaredCommands = manifest
    ? new Set((manifest.contributes?.commands ?? []).map((c) => c.id))
    : undefined;
  const declaredPanels = manifest
    ? new Set((manifest.contributes?.panels ?? []).map((p) => p.id))
    : undefined;
  const messages = new Map((options.messages ?? []).map((m) => [m.id, m]));
  const bodies = options.bodies ?? {};
  let settings: Record<string, JsonValue> = { ...options.settings };
  const settingsListeners: ((values: Record<string, JsonValue>) => void)[] = [];
  const quota = options.storageQuota ?? 5 * 1024 * 1024;
  const dialogs: PluginDialogSpec[] = [];
  const notifications: PluginNotificationSpec[] = [];
  const panelUpdates: { name: string; tree: PluginUiNode[] }[] = [];
  const logs: TestLogLine[] = [];
  const storage = new Map<string, JsonValue>();
  const commands = new Map<string, PluginCommandHandler>();
  const panels = new Map<string, PluginPanelProvider>();

  const need = (permission: string): void => {
    if (!granted.has(permission)) {
      throw createPluginApiError("forbidden", `permission ${permission} is not granted`);
    }
  };
  const checked = (nodes: PluginUiNode[], root: string): void => {
    const issues = checkTree(nodes, root);
    if (issues.length > 0) {
      throw createPluginApiError(
        "invalid_request",
        issues.map((i) => `${i.path}: ${i.message}`).join("; "),
      );
    }
  };
  const used = (): number => {
    let n = 0;
    for (const [k, v] of storage) n += k.length + JSON.stringify(v).length;
    return n;
  };
  const register = <T>(
    kind: string,
    map: Map<string, T>,
    declared: Set<string> | undefined,
    name: string,
    value: T,
  ): void => {
    if (declared && !declared.has(name)) {
      throw createPluginApiError("invalid_request", `${kind} ${name} is not in the manifest`);
    }
    if (map.has(name)) throw createPluginApiError("conflict", `${kind} ${name} is registered`);
    map.set(name, value);
  };
  const registerCommand = (name: string, handler: PluginCommandHandler): void =>
    register("command", commands, declaredCommands, name, handler);
  const registerPanel = (name: string, panel: PluginPanelProvider | PluginPanelRender): void =>
    register(
      "panel",
      panels,
      declaredPanels,
      name,
      typeof panel === "function" ? { render: panel } : panel,
    );
  const log =
    (level: PluginLogLevel) =>
    (...args: unknown[]): void => {
      logs.push({ level, message: format(args) });
    };
  const settingsApi: NailApi["settings"] = {
    get: <T extends JsonValue = JsonValue>(key: string) => settings[key] as T | undefined,
    all: () => ({ ...settings }),
    onChange: (listener) => {
      settingsListeners.push(listener);
    },
  };

  const api: NailApi = {
    plugin,
    apiVersion: "1",
    mail: {
      async get(id) {
        need("mail.read");
        const message = messages.get(id);
        if (!message) throw createPluginApiError("not_found", `message ${id}`);
        return message;
      },
      async getBody(id) {
        need("mail.read");
        need("mail.read.body");
        const message = messages.get(id);
        const body = bodies[id];
        if (!message || !body) throw createPluginApiError("not_found", `message ${id}`);
        if (message.encrypted) {
          throw createPluginApiError("forbidden", "end-to-end encrypted mail");
        }
        return body;
      },
      async search(query, searchOptions = {}) {
        need("mail.read");
        const words = query.toLowerCase().split(/\s+/).filter(Boolean);
        return [...messages.values()]
          .filter((m) => words.every((w) => m.subject.toLowerCase().includes(w)))
          .sort((a, b) => b.date.localeCompare(a.date))
          .slice(0, searchOptions.limit ?? 50);
      },
      async thread(threadId) {
        need("mail.read");
        return [...messages.values()]
          .filter((m) => m.threadId === threadId)
          .sort((a, b) => a.date.localeCompare(b.date));
      },
    },
    ui: {
      registerCommand,
      registerPanel,
      async showDialog(spec) {
        need("ui.dialogs");
        checked(spec.body, "body");
        dialogs.push(spec);
        return options.dialog ? options.dialog(spec) : { button: null, values: {} };
      },
      async notify(notification) {
        need("notifications");
        const spec = typeof notification === "string" ? { title: notification } : notification;
        if (!spec.title.trim() || spec.title.length > 200 || (spec.body?.length ?? 0) > 2000) {
          throw createPluginApiError(
            "invalid_request",
            "the notification title or body is empty or too long",
          );
        }
        notifications.push(spec);
      },
      async updatePanel(name, tree) {
        if (!panels.has(name)) throw createPluginApiError("not_found", `panel ${name}`);
        checked(tree, name);
        panelUpdates.push({ name, tree });
      },
    },
    commands: { register: registerCommand },
    storage: {
      async get<T extends JsonValue = JsonValue>(key: string) {
        need("storage");
        const value = storage.get(key);
        return (value === undefined ? null : clone(value)) as T | null;
      },
      async set(key, value) {
        need("storage");
        if (key.length === 0 || key.length > 256) {
          throw createPluginApiError("invalid_request", "storage keys are 1–256 bytes");
        }
        const before = storage.get(key);
        storage.set(key, clone(value));
        if (used() > quota) {
          if (before === undefined) storage.delete(key);
          else storage.set(key, before);
          throw createPluginApiError("too_large", "storage quota exceeded");
        }
      },
      async delete(key) {
        need("storage");
        return storage.delete(key);
      },
      async keys(prefix) {
        need("storage");
        return [...storage.keys()].filter((k) => !prefix || k.startsWith(prefix)).sort();
      },
      async clear() {
        need("storage");
        storage.clear();
      },
      async usage(): Promise<PluginStorageUsage> {
        need("storage");
        return { keys: storage.size, usedBytes: used(), quotaBytes: quota };
      },
    },
    http: {
      async fetch(url, request = {}) {
        const parsed = parseUrl(url);
        const allowed = [...granted].some(
          (p) => p.startsWith("network:") && hostAllowed(p.slice(8), parsed.hostname),
        );
        if (!allowed) {
          throw createPluginApiError("forbidden", `permission network:${parsed.hostname}`);
        }
        if (parsed.protocol !== "https:") {
          throw createPluginApiError("invalid_request", `scheme ${parsed.protocol} is not allowed`);
        }
        const res = options.fetch ? await options.fetch(url, request) : { status: 404 };
        const status = res.status ?? 200;
        const body = res.body ?? "";
        return {
          status,
          ok: status >= 200 && status < 300,
          headers: res.headers ?? {},
          body,
          text: async () => body,
          json: async () => JSON.parse(body),
        };
      },
    },
    settings: settingsApi,
    log: {
      debug: log("debug"),
      info: log("info"),
      warn: log("warn"),
      error: log("error"),
    },
  };

  return {
    api,
    dialogs,
    notifications,
    panelUpdates,
    logs,
    storage,
    commandNames: () => [...commands.keys()],
    panelNames: () => [...panels.keys()],
    install() {
      const g = globalThis as { nail?: NailApi };
      const previous = g.nail;
      g.nail = api;
      return () => {
        if (previous === undefined) delete g.nail;
        else g.nail = previous;
      };
    },
    activationContext: () => ({
      plugin,
      commands: { register: registerCommand },
      panels: { register: registerPanel },
      settings: settingsApi,
    }),
    async runCommand(name, ctx) {
      const handler = commands.get(name);
      if (!handler) throw createPluginApiError("not_found", `command ${name}`);
      await handler(commandContext(ctx));
    },
    async renderPanel(name, ctx) {
      const provider = panels.get(name);
      if (!provider) throw createPluginApiError("not_found", `panel ${name}`);
      const tree = await provider.render(commandContext(ctx));
      checked(tree, name);
      return tree;
    },
    async clickPanelButton(name, componentId, values = {}) {
      const provider = panels.get(name);
      if (!provider) throw createPluginApiError("not_found", `panel ${name}`);
      await provider.onAction?.({ componentId, values });
    },
    changeSettings(values) {
      settings = { ...settings, ...values };
      for (const listener of settingsListeners) listener({ ...settings });
    },
  };
}
