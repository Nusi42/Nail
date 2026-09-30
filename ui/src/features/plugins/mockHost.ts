/**
 * A simulated plug-in host for the mock core (UI tests and the mock dev
 * server): packages that can be previewed and installed, and plug-ins whose
 * behaviour is written in TypeScript against a small API shaped like the
 * real `nail.*` preview API — dialogs that resolve with the user's answer,
 * notifications, storage, logs, panels — behind the same permission gate
 * (a call without its grant is refused and logged, like the host does).
 *
 * The built-in packages (`MOCK_PACKAGES`) are the design's "Follow up"
 * example adapted to the preview (a command that asks for a date in a
 * dialog, stores a reminder and notifies) and a package whose manifest is
 * broken.
 */
import type {
  ConfigMergePreview,
  JsonValue,
  PluginCommandSurface,
  PluginDialogResult,
  PluginDialogSpec,
  PluginInfo,
  PluginManifest,
  PluginManifestIssue,
  PluginMessage,
  PluginNotificationSpec,
  PluginPackageSource,
  PluginPermission,
  PluginStopReason,
  PluginUiNode,
  PluginUiTarget,
  UiContext,
} from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";
import { answerCustomization } from "../customization/mock";
import { parsePluginCommandId, parsePluginPanelId } from "./ids";
import { appendLog, checkGrants, stateOf, type HostRequest } from "./mock";
import { placementText } from "./placementText";

/** What a command handler receives. */
export interface MockCommandCall {
  surface: PluginCommandSurface;
  context: UiContext;
  clientId: string | null;
}

/** The API a mock plug-in's behaviour calls (permission-gated). */
export interface MockPluginApi {
  readonly pluginId: string;
  showDialog(spec: PluginDialogSpec, clientId: string | null): Promise<PluginDialogResult>;
  notify(spec: PluginNotificationSpec, clientId: string | null): void;
  storageGet(key: string): JsonValue | undefined;
  storageSet(key: string, value: JsonValue): void;
  storageKeys(): string[];
  storageDelete(key: string): void;
  log(level: "debug" | "info" | "warn" | "error", message: string): void;
  /** Re-renders a panel (`PluginPanelChanged`). */
  setPanel(panel: string, tree: PluginUiNode[]): void;
  /** A message as the plug-in sees it (`mail.read`). */
  message(id: string): PluginMessage | null;
}

/** How a mock plug-in behaves. */
export interface MockPluginBehaviour {
  commands?: Record<string, (api: MockPluginApi, call: MockCommandCall) => Promise<void> | void>;
  panels?: Record<string, (api: MockPluginApi, context: UiContext) => PluginUiNode[]>;
  onAction?(
    api: MockPluginApi,
    target: PluginUiTarget,
    componentId: string,
    values: Record<string, JsonValue>,
    clientId: string | null,
  ): Promise<void> | void;
}

/** A package the mock host can install. */
export interface MockPackage {
  manifest: PluginManifest | null;
  issues: PluginManifestIssue[];
  sha256: string;
  size: number;
  behaviour: MockPluginBehaviour;
}

interface PendingDialog {
  pluginId: string;
  resolve(result: PluginDialogResult): void;
}

export interface MockHost {
  /** Packages by source (`file:<path>`, `url:<url>`, `dev:<folder>`). */
  packages: Record<string, MockPackage>;
  /** Behaviour of installed plug-ins, by plug-in id. */
  behaviours: Record<string, MockPluginBehaviour>;
  /** Plug-in storage, by plug-in id. */
  storage: Record<string, Record<string, JsonValue>>;
  /** Open dialogs by id. */
  dialogs: Map<string, PendingDialog>;
  /** Stops a plug-in's runtime: its dialogs close (`PluginDialogClosed`). */
  stopped(core: MockCore, pluginId: string, reason: PluginStopReason): void;
}

/** The key of a package source. */
export function sourceKey(source: PluginPackageSource): string {
  switch (source.kind) {
    case "file":
      return `file:${source.path}`;
    case "url":
      return `url:${source.url}`;
    case "dev":
      return `dev:${source.folder}`;
  }
}

/** The "Follow up" example's manifest. */
export function followUpManifest(): PluginManifest {
  return {
    id: "follow-up",
    name: "Follow-up",
    version: "1.0.0",
    apiVersion: "1",
    main: "dist/index.js",
    description: "Reminds you to follow up on a message.",
    author: "Nail examples",
    permissions: ["mail.read", "ui.dialogs", "notifications", "storage"],
    contributes: {
      commands: [{ id: "remind", title: "Follow up", icon: "flag", contexts: ["list", "reader"] }],
      placements: [
        { toolbar: "reader", command: "remind", after: "mail.forward" },
        { panel: "reminders", after: "message-view" },
      ],
      keys: [{ key: "shift+r", command: "remind", context: "list" }],
      panels: [{ id: "reminders", title: "Reminders" }],
      settings: [
        { type: "number", key: "days", title: "Default days", default: 1, min: 0, max: 30 },
      ],
    },
  };
}

function remindersTree(api: MockPluginApi): PluginUiNode[] {
  const keys = api.storageKeys();
  if (keys.length === 0) return [{ type: "text", value: "No reminders.", style: "muted" }];
  return [
    {
      type: "table",
      columns: [{ label: "Message" }, { label: "Due" }],
      rows: keys.map((key) => {
        const value = api.storageGet(key);
        const due =
          value &&
          typeof value === "object" &&
          !Array.isArray(value) &&
          typeof value.due === "string"
            ? value.due
            : "";
        return [key, due];
      }),
    },
    { type: "button", id: "clear", label: "Clear reminders", variant: "danger" },
  ];
}

/** The "Follow up" example's behaviour. */
export const followUpBehaviour: MockPluginBehaviour = {
  commands: {
    remind: async (api, call) => {
      const id = call.context.message_id ?? call.context.selected_message_ids[0];
      if (!id) return;
      const message = api.message(id);
      const result = await api.showDialog(
        {
          title: "Follow up",
          body: [
            { type: "text", value: `Remind me about “${message?.subject ?? id}”` },
            { type: "date", id: "due", label: "Due", default: "tomorrow", required: true },
            { type: "textarea", id: "note", label: "Note", maxLength: 200 },
          ],
          buttons: [
            { id: "ok", label: "Save reminder", primary: true },
            { id: "cancel", label: "Cancel" },
          ],
        },
        call.clientId,
      );
      if (result.button !== "ok") return;
      api.storageSet(id, { due: result.values.due ?? null, note: result.values.note ?? "" });
      api.notify({ title: "Reminder saved", level: "success" }, call.clientId);
      api.setPanel("reminders", remindersTree(api));
    },
  },
  panels: { reminders: (api) => remindersTree(api) },
  onAction: (api, target, componentId) => {
    if (target.kind !== "panel" || componentId !== "clear") return;
    for (const key of api.storageKeys()) api.storageDelete(key);
    api.setPanel("reminders", remindersTree(api));
  },
};

/** The packages the mock host knows. */
export function mockPackages(): Record<string, MockPackage> {
  const followUp: MockPackage = {
    manifest: followUpManifest(),
    issues: [],
    sha256: "5f0c".padEnd(64, "0"),
    size: 4096,
    behaviour: followUpBehaviour,
  };
  return {
    "file:C:/plugins/follow-up.nailplugin": followUp,
    "url:https://plugins.example/follow-up.nailplugin": followUp,
    "dev:C:/dev/follow-up": { ...followUp, sha256: "de70".padEnd(64, "0") },
    "file:C:/plugins/broken.nailplugin": {
      manifest: null,
      issues: [{ kind: "invalid-json", path: "", detail: "expected value at line 1 column 1" }],
      sha256: "bad0".padEnd(64, "0"),
      size: 12,
      behaviour: {},
    },
  };
}

export function createMockHost(): MockHost {
  const host: MockHost = {
    packages: mockPackages(),
    behaviours: {},
    storage: {},
    dialogs: new Map(),
    stopped(core, pluginId) {
      for (const [dialogId, pending] of [...host.dialogs]) {
        if (pending.pluginId !== pluginId) continue;
        host.dialogs.delete(dialogId);
        pending.resolve({ button: null, values: {} });
        core.emit({ type: "PluginDialogClosed", dialog_id: dialogId, plugin_id: pluginId });
      }
    },
  };
  return host;
}

/** A permission refused by the gate. */
export class PermissionDenied extends Error {
  constructor(readonly permission: PluginPermission) {
    super(`permission denied: ${permission}`);
  }
}

function granted(plugin: PluginInfo, permission: PluginPermission): boolean {
  return plugin.grants.some((g) => g.permission === permission);
}

/** The gated API of an installed plug-in. */
export function pluginApi(core: MockCore, host: MockHost, plugin: PluginInfo): MockPluginApi {
  const gate = (permission: PluginPermission) => {
    if (!granted(plugin, permission)) {
      appendLog(core, plugin.id, "error", "host", `permission denied: ${permission}`);
      throw new PermissionDenied(permission);
    }
  };
  const store = () => (host.storage[plugin.id] ??= {});
  return {
    pluginId: plugin.id,
    showDialog(spec, clientId) {
      gate("ui.dialogs");
      const dialogId = core.nextId("plugin-dialog-");
      return new Promise((resolve) => {
        host.dialogs.set(dialogId, { pluginId: plugin.id, resolve });
        core.emit({
          type: "PluginDialogOpened",
          dialog: {
            dialog_id: dialogId,
            plugin_id: plugin.id,
            client_id: clientId,
            spec: structuredClone(spec),
          },
        });
      });
    },
    notify(spec, clientId) {
      gate("notifications");
      core.emit({
        type: "PluginNotification",
        notification: {
          id: core.nextId("plugin-notification-"),
          plugin_id: plugin.id,
          client_id: clientId,
          spec: structuredClone(spec),
        },
      });
    },
    storageGet(key) {
      gate("storage");
      return store()[key];
    },
    storageSet(key, value) {
      gate("storage");
      store()[key] = structuredClone(value);
    },
    storageKeys() {
      gate("storage");
      return Object.keys(store()).sort();
    },
    storageDelete(key) {
      gate("storage");
      host.storage[plugin.id] = Object.fromEntries(
        Object.entries(store()).filter(([k]) => k !== key),
      );
    },
    log(level, message) {
      appendLog(core, plugin.id, level, "plugin", message);
    },
    setPanel(panel, tree) {
      core.emit({
        type: "PluginPanelChanged",
        panel: `${plugin.id}.${panel}`,
        tree: structuredClone(tree),
      });
    },
    message(id) {
      gate("mail.read");
      const m = core.data.messages.find((x) => x.id === id);
      if (!m) return null;
      return {
        id: m.id,
        uri: `nail://messages/${m.id}`,
        accountId: m.account_id,
        folderId: m.folder_id,
        ...(m.thread_id ? { threadId: m.thread_id } : {}),
        subject: m.subject,
        ...(m.from ? { from: m.from } : {}),
        to: structuredClone(m.to),
        cc: [],
        date: m.date,
        flags: structuredClone(m.flags),
        tags: [...m.tags],
        hasAttachments: m.has_attachments,
        encrypted: false,
      };
    },
  };
}

/** Runs plug-in code: permission errors and exceptions end up in its log. */
function runPluginCode(core: MockCore, pluginId: string, work: () => Promise<void> | void): void {
  core.later(() => {
    void (async () => {
      try {
        await work();
      } catch (error) {
        if (error instanceof PermissionDenied) return;
        appendLog(core, pluginId, "error", "host", String(error));
      }
    })();
  }, 0);
}

function installed(core: MockCore, pkg: MockPackage, source: PluginPackageSource): PluginInfo {
  const state = core.data.features.plugins;
  const manifest = pkg.manifest as PluginManifest;
  const now = new Date().toISOString();
  const existing = state.plugins.find((p) => p.id === manifest.id);
  const info: PluginInfo = {
    id: manifest.id,
    manifest: structuredClone(manifest),
    source: structuredClone(source),
    enabled: true,
    state: "running",
    grants: [],
    storage: { keys: 0, used_bytes: 0, quota_bytes: 5 * 1024 * 1024 },
    package_sha256: source.kind === "dev" ? null : pkg.sha256,
    installed_at: existing?.installed_at ?? now,
    updated_at: now,
    error: null,
  };
  state.plugins = [...state.plugins.filter((p) => p.id !== info.id), info];
  return info;
}

function runnable(plugin: PluginInfo): boolean {
  return plugin.enabled && plugin.error === null;
}

export function answerHost(core: MockCore, host: MockHost, request: HostRequest) {
  const state = core.data.features.plugins;
  const find = (id: string) => state.plugins.find((p) => p.id === id);
  switch (request.type) {
    case "PreviewPluginInstall": {
      const pkg = host.packages[sourceKey(request.source)];
      if (!pkg)
        return errorResponse("not_found", `no plug-in package at ${sourceKey(request.source)}`);
      const current = pkg.manifest ? find(pkg.manifest.id) : undefined;
      return {
        type: "PluginInstallPreview" as const,
        preview: {
          source: structuredClone(request.source),
          manifest: structuredClone(pkg.manifest),
          issues: structuredClone(pkg.issues),
          package_sha256: pkg.sha256,
          size: pkg.size,
          installed_version: current?.manifest.version ?? null,
        },
      };
    }
    case "InstallPlugin":
    case "LoadDevPlugin": {
      const source: PluginPackageSource =
        request.type === "LoadDevPlugin" ? { kind: "dev", folder: request.folder } : request.source;
      const pkg = host.packages[sourceKey(source)];
      if (!pkg) return errorResponse("not_found", `no plug-in package at ${sourceKey(source)}`);
      if (request.type === "InstallPlugin" && request.package_sha256 !== pkg.sha256) {
        return errorResponse("conflict", "the package changed since it was previewed");
      }
      if (!pkg.manifest || pkg.issues.length > 0) {
        return errorResponse("invalid_request", "the package has problems");
      }
      const draft = { ...installed(core, pkg, source) };
      const problem = checkGrants(draft, request.grants);
      if (problem) {
        state.plugins = state.plugins.filter((p) => p.id !== draft.id);
        return errorResponse("invalid_request", problem);
      }
      const plugin = find(draft.id) as PluginInfo;
      plugin.grants = structuredClone(request.grants);
      plugin.enabled = request.type === "LoadDevPlugin" ? true : request.enable;
      plugin.state = stateOf(state, plugin);
      host.behaviours[plugin.id] = pkg.behaviour;
      appendLog(core, plugin.id, "info", "host", `installed ${plugin.manifest.version}`);
      core.emit({ type: "PluginsChanged" });
      return { type: "Plugin" as const, plugin: structuredClone(plugin) };
    }
    case "PreviewPluginPlacements": {
      const plugin = find(request.plugin_id);
      if (!plugin) return errorResponse("not_found", `plug-in not found: ${request.plugin_id}`);
      const text = placementText(core.data.layout, plugin);
      if (text === null) {
        const empty: ConfigMergePreview = { file: "layout", sections: [], errors: [] };
        return {
          type: "PluginPlacements" as const,
          plugin_id: plugin.id,
          incoming: "",
          preview: empty,
        };
      }
      const r = answerCustomization(core, {
        type: "PreviewConfigMerge",
        file: "layout",
        incoming: text,
      });
      if (r.type !== "ConfigMergePreview") return r;
      return {
        type: "PluginPlacements" as const,
        plugin_id: plugin.id,
        incoming: text,
        preview: r.preview,
      };
    }
    case "RunPluginCommand": {
      const parsed = parsePluginCommandId(request.command);
      const plugin = parsed ? find(parsed.plugin) : undefined;
      const behaviour = plugin ? host.behaviours[plugin.id] : undefined;
      const handler = parsed ? behaviour?.commands?.[parsed.command] : undefined;
      if (!plugin || !parsed)
        return errorResponse("not_found", `unknown command ${request.command}`);
      if (!runnable(plugin)) return errorResponse("unavailable", `${plugin.id} is not running`);
      if (!handler) return errorResponse("not_found", `unknown command ${request.command}`);
      const api = pluginApi(core, host, plugin);
      const call: MockCommandCall = {
        surface: request.surface,
        context: structuredClone(request.context),
        clientId: request.client_id,
      };
      runPluginCode(core, plugin.id, () => handler(api, call));
      return { type: "Done" as const };
    }
    case "RespondPluginDialog": {
      const pending = host.dialogs.get(request.dialog_id);
      if (!pending) return errorResponse("not_found", `no dialog ${request.dialog_id}`);
      host.dialogs.delete(request.dialog_id);
      core.emit({
        type: "PluginDialogClosed",
        dialog_id: request.dialog_id,
        plugin_id: pending.pluginId,
      });
      pending.resolve({ button: request.button, values: structuredClone(request.values) });
      return { type: "Done" as const };
    }
    case "PluginUiAction": {
      const pluginId =
        request.target.kind === "dialog"
          ? host.dialogs.get(request.target.dialog_id)?.pluginId
          : parsePluginPanelId(request.target.panel)?.plugin;
      const plugin = pluginId === undefined ? undefined : find(pluginId);
      if (!plugin) return errorResponse("not_found", "no such dialog or panel");
      if (!runnable(plugin)) return errorResponse("unavailable", `${plugin.id} is not running`);
      const onAction = host.behaviours[plugin.id]?.onAction;
      if (onAction) {
        const api = pluginApi(core, host, plugin);
        const { target, component_id: componentId, values, client_id: clientId } = request;
        runPluginCode(core, plugin.id, () =>
          onAction(api, target, componentId, structuredClone(values), clientId),
        );
      }
      return { type: "Done" as const };
    }
    case "GetPluginPanel": {
      const parsed = parsePluginPanelId(request.panel);
      const plugin = parsed ? find(parsed.plugin) : undefined;
      if (!plugin || !parsed) return errorResponse("not_found", `no panel ${request.panel}`);
      if (!runnable(plugin)) return errorResponse("unavailable", `${plugin.id} is not running`);
      const render = host.behaviours[plugin.id]?.panels?.[parsed.panel];
      let tree: PluginUiNode[] = [];
      try {
        tree = render ? render(pluginApi(core, host, plugin), request.context) : [];
      } catch (error) {
        if (!(error instanceof PermissionDenied)) {
          appendLog(core, plugin.id, "error", "host", String(error));
        }
      }
      return { type: "PluginPanel" as const, panel: request.panel, tree };
    }
  }
}

/** Test helper: the host stops a plug-in after a crash (`PluginStopped`). */
export function crashPlugin(
  core: MockCore,
  pluginId: string,
  reason: Extract<PluginStopReason, "error" | "cpu-budget" | "memory-limit">,
): void {
  const state = core.data.features.plugins;
  const plugin = state.plugins.find((p) => p.id === pluginId);
  if (!plugin) return;
  plugin.error = reason === "error" ? "the plug-in threw an error" : `stopped: ${reason}`;
  plugin.localized = { id: `plugin-stopped-${reason}`, args: { plugin: plugin.manifest.name } };
  plugin.state = "crashed";
  state.host?.stopped(core, pluginId, reason);
  appendLog(core, pluginId, "error", "host", plugin.error);
  core.emit({
    type: "PluginStopped",
    plugin_id: pluginId,
    reason,
    error: plugin.error,
    localized: { id: `plugin-stopped-${reason}`, args: { plugin: plugin.manifest.name } },
  });
  core.emit({ type: "PluginsChanged" });
}
