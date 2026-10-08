/**
 * The mock core's answers to the M2d plug-in requests (ADR 0050), used by
 * `MockTransport` (`../../api/mock.ts` routes exactly these requests here).
 *
 * The administration of installed plug-ins (listing, enabling, grants,
 * settings, logs, uninstalling) works on `data.features.plugins` like the
 * core's store. The plug-in host is simulated by `./mockHost.ts` (packages
 * to preview and install, commands, dialogs, notifications, panels, the
 * permission gate); with `host: null` everything that needs the host
 * answers `unsupported`, as the core did before WP64.
 */
import type {
  JsonValue,
  PluginGrant,
  PluginInfo,
  PluginLogEntry,
  PluginSettingDef,
  Request,
  Response,
} from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";
import { takesAccounts as permissionTakesAccounts } from "./grants";
import { createExtensionsMockState, type ExtensionsMockState } from "./extensionsMock";
import { answerHost, createMockHost, type MockHost } from "./mockHost";
import { charCount } from "../../text/chars";

/** The requests this feature answers. */
export const PLUGINS_REQUESTS = [
  "ListPlugins",
  "GetPlugin",
  "PreviewPluginInstall",
  "InstallPlugin",
  "LoadDevPlugin",
  "EnablePlugin",
  "DisablePlugin",
  "UninstallPlugin",
  "UpdatePluginGrants",
  "ListPluginLogs",
  "ClearPluginLogs",
  "GetPluginSettings",
  "UpdatePluginSettings",
  "PreviewPluginPlacements",
  "RunPluginCommand",
  "RespondPluginDialog",
  "PluginUiAction",
  "GetPluginPanel",
] as const satisfies readonly Request["type"][];

export type PluginsRequest = Extract<Request, { type: (typeof PLUGINS_REQUESTS)[number] }>;

/** The requests that need the plug-in host. */
export const HOST_REQUESTS = [
  "PreviewPluginInstall",
  "InstallPlugin",
  "LoadDevPlugin",
  "PreviewPluginPlacements",
  "RunPluginCommand",
  "RespondPluginDialog",
  "PluginUiAction",
  "GetPluginPanel",
] as const satisfies readonly PluginsRequest["type"][];

export type HostRequest = Extract<Request, { type: (typeof HOST_REQUESTS)[number] }>;

export function isPluginsRequest(request: Request): request is PluginsRequest {
  return (PLUGINS_REQUESTS as readonly string[]).includes(request.type);
}

function isHostRequest(request: PluginsRequest): request is HostRequest {
  return (HOST_REQUESTS as readonly string[]).includes(request.type);
}

/** The mock's plug-ins (tests put installed plug-ins here). */
export interface PluginsMockState {
  plugins: PluginInfo[];
  logs: PluginLogEntry[];
  /** Values the user changed, per plug-in id. */
  settings: Record<string, Record<string, JsonValue>>;
  /** The simulated plug-in host; `null` = requests needing it are `unsupported`. */
  host: MockHost | null;
  /** Next log sequence number. */
  nextSeq: number;
  /** Hooks, extensions, jobs and vetoes (M4a; `./extensionsMock.ts`). */
  extensions: ExtensionsMockState;
}

export function createPluginsMockState(): PluginsMockState {
  return {
    plugins: [],
    logs: [],
    settings: {},
    host: createMockHost(),
    nextSeq: 1,
    extensions: createExtensionsMockState(),
  };
}

/** The value a setting has until the user changes it (like the core). */
export function defaultSettingValue(def: PluginSettingDef): JsonValue {
  switch (def.type) {
    case "string":
      return def.default ?? "";
    case "number":
      return Math.max(def.default ?? def.min ?? 0, def.min ?? Number.MIN_SAFE_INTEGER);
    case "boolean":
      return def.default ?? false;
    case "select":
      return def.default ?? def.options[0]?.value ?? "";
  }
}

/** Whether `value` is valid for the setting (like the core). */
export function acceptsSetting(def: PluginSettingDef, value: JsonValue): boolean {
  switch (def.type) {
    case "string":
      return (
        typeof value === "string" && charCount(value) <= Math.min(def.maxLength ?? 10_000, 10_000)
      );
    case "number":
      return (
        typeof value === "number" &&
        Number.isInteger(value) &&
        (def.min === undefined || value >= def.min) &&
        (def.max === undefined || value <= def.max)
      );
    case "boolean":
      return typeof value === "boolean";
    case "select":
      return typeof value === "string" && def.options.some((o) => o.value === value);
  }
}

/** Every setting's current value (defaults filled in). */
export function settingsOf(state: PluginsMockState, plugin: PluginInfo): Record<string, JsonValue> {
  const values: Record<string, JsonValue> = {};
  const changed = state.settings[plugin.id] ?? {};
  for (const def of plugin.manifest.contributes?.settings ?? []) {
    values[def.key] = changed[def.key] ?? defaultSettingValue(def);
  }
  return values;
}

/** The run state of a plug-in (running only with a host). */
export function stateOf(state: PluginsMockState, plugin: PluginInfo): PluginInfo["state"] {
  if (!plugin.enabled) return "disabled";
  if (plugin.error !== null) return "crashed";
  return state.host ? "running" : "stopped";
}

/** The problem with `grants` for `plugin` (like `nail_store::check_plugin_grants`). */
export function checkGrants(plugin: PluginInfo, grants: PluginGrant[]): string | null {
  const declared = plugin.manifest.permissions ?? [];
  const seen = new Set<string>();
  for (const grant of grants) {
    if (!declared.includes(grant.permission)) {
      return `permission ${grant.permission} is not declared by plug-in ${plugin.id}`;
    }
    if (seen.has(grant.permission)) return `permission ${grant.permission} granted twice`;
    seen.add(grant.permission);
    const takesAccounts = permissionTakesAccounts(grant.permission);
    if (grant.accounts && (!takesAccounts || grant.accounts.length === 0)) {
      return `permission ${grant.permission} cannot be limited to these accounts`;
    }
  }
  return null;
}

/** Appends a log line and announces it (`PluginLogLine`). */
export function appendLog(
  core: MockCore,
  pluginId: string,
  level: PluginLogEntry["level"],
  source: PluginLogEntry["source"],
  message: string,
): PluginLogEntry {
  const state = core.data.features.plugins;
  const entry: PluginLogEntry = {
    seq: state.nextSeq++,
    plugin_id: pluginId,
    at: new Date().toISOString(),
    level,
    source,
    message,
  };
  state.logs.push(entry);
  core.emit({ type: "PluginLogLine", entry: structuredClone(entry) });
  return entry;
}

export function answerPlugins(core: MockCore, request: PluginsRequest): Response {
  const state = core.data.features.plugins;
  const find = (id: string) => state.plugins.find((p) => p.id === id);
  const notFound = (id: string) => errorResponse("not_found", `plug-in not found: ${id}`);
  const changed = (plugin: PluginInfo): Response => {
    plugin.state = stateOf(state, plugin);
    core.emit({ type: "PluginsChanged" });
    return { type: "Plugin", plugin: structuredClone(plugin) };
  };
  if (isHostRequest(request)) {
    if (!state.host) {
      return errorResponse(
        "unsupported",
        `${request.type} needs the plug-in host (M2d WP61 / WP64)`,
      );
    }
    return answerHost(core, state.host, request);
  }
  switch (request.type) {
    case "ListPlugins":
      return {
        type: "Plugins",
        plugins: structuredClone([...state.plugins].sort((a, b) => a.id.localeCompare(b.id))),
      };
    case "GetPlugin": {
      const plugin = find(request.plugin_id);
      return plugin
        ? { type: "Plugin", plugin: structuredClone(plugin) }
        : notFound(request.plugin_id);
    }
    case "EnablePlugin":
    case "DisablePlugin": {
      const plugin = find(request.plugin_id);
      if (!plugin) return notFound(request.plugin_id);
      plugin.enabled = request.type === "EnablePlugin";
      if (plugin.enabled) {
        plugin.error = null;
        delete plugin.localized;
      } else if (state.host) {
        state.host.stopped(core, plugin.id, "disabled");
      }
      return changed(plugin);
    }
    case "UninstallPlugin": {
      if (!find(request.plugin_id)) return notFound(request.plugin_id);
      state.host?.stopped(core, request.plugin_id, "uninstalled");
      state.plugins = state.plugins.filter((p) => p.id !== request.plugin_id);
      state.logs = state.logs.filter((l) => l.plugin_id !== request.plugin_id);
      state.settings = Object.fromEntries(
        Object.entries(state.settings).filter(([id]) => id !== request.plugin_id),
      );
      core.emit({ type: "PluginsChanged" });
      return { type: "Done" };
    }
    case "UpdatePluginGrants": {
      const plugin = find(request.plugin_id);
      if (!plugin) return notFound(request.plugin_id);
      const problem = checkGrants(plugin, request.grants);
      if (problem) return errorResponse("invalid_request", problem);
      plugin.grants = structuredClone(request.grants);
      return changed(plugin);
    }
    case "ListPluginLogs": {
      if (request.plugin_id && !find(request.plugin_id)) return notFound(request.plugin_id);
      const limit = Math.min(Math.max(request.limit, 1), 1000);
      const entries = state.logs.filter(
        (l) =>
          (request.plugin_id === null || l.plugin_id === request.plugin_id) &&
          l.seq > (request.after_seq ?? 0),
      );
      return { type: "PluginLogs", entries: structuredClone(entries.slice(-limit)) };
    }
    case "ClearPluginLogs":
      if (!find(request.plugin_id)) return notFound(request.plugin_id);
      state.logs = state.logs.filter((l) => l.plugin_id !== request.plugin_id);
      return { type: "Done" };
    case "GetPluginSettings":
    case "UpdatePluginSettings": {
      const plugin = find(request.plugin_id);
      if (!plugin) return notFound(request.plugin_id);
      const schema = plugin.manifest.contributes?.settings ?? [];
      if (request.type === "UpdatePluginSettings") {
        const invalid = Object.entries(request.values)
          .filter(([key, value]) => {
            const def = schema.find((d) => d.key === key);
            return !def || !acceptsSetting(def, value);
          })
          .map(([key]) => key)
          .sort();
        if (invalid.length > 0) {
          return {
            type: "Error",
            code: "invalid_request",
            message: `invalid plug-in settings: ${invalid.join(", ")}`,
            localized: { id: "plugin-settings-invalid", args: { keys: invalid.join(", ") } },
          };
        }
        state.settings[plugin.id] = {
          ...state.settings[plugin.id],
          ...request.values,
        };
      }
      return {
        type: "PluginSettings",
        plugin_id: plugin.id,
        schema: structuredClone(schema),
        values: settingsOf(state, plugin),
      };
    }
  }
}
