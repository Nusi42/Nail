/**
 * The mock core's answers to the M4b desktop requests (ADR 0087), used by
 * `MockTransport` (`../../api/mock.ts` routes exactly these requests here).
 *
 * Like the core with a desktop host: settings are stored in
 * `data.features.system`; the host's side (installation facts, updater
 * state, Run key, default apps) is simulated there too. The policy wins
 * (a disabled updater reports `disabled` and refuses checks with
 * `forbidden`; a forced channel cannot be changed), checks / downloads /
 * installs follow the state machine (`conflict` otherwise). Tests and demos
 * move the updater with `setUpdateState`. With `desktop: false` everything
 * but the first run answers `unsupported` like `nail serve`.
 */
import type {
  DefaultAppsStatus,
  DesktopSettings,
  FirstRunState,
  InstallationInfo,
  Request,
  Response,
  UpdateSettings,
  UpdateState,
  UpdateStatus,
} from "../../api/generated";
import type { MockCore } from "../../api/mockMail";
import { DEFAULT_APP_TARGETS, canCheck, canDownload, canInstall } from "./model";

/** The requests this feature answers. */
export const SYSTEM_REQUESTS = [
  "GetInstallation",
  "GetUpdateStatus",
  "SetUpdateSettings",
  "CheckForUpdates",
  "DownloadUpdate",
  "InstallUpdate",
  "GetDesktopSettings",
  "SetDesktopSettings",
  "GetDefaultApps",
  "OpenDefaultAppsSettings",
  "GetFirstRun",
  "CompleteFirstRun",
] as const satisfies readonly Request["type"][];

export type SystemRequest = Extract<Request, { type: (typeof SYSTEM_REQUESTS)[number] }>;

export function isSystemRequest(request: Request): request is SystemRequest {
  return (SYSTEM_REQUESTS as readonly string[]).includes(request.type);
}

/** Mock-core state of this feature (core store + simulated desktop host). */
export interface SystemMockState {
  /** Whether a desktop host exists (false = `nail serve`). */
  desktop: boolean;
  installation: InstallationInfo;
  updateSettings: UpdateSettings;
  updateState: UpdateState;
  lastCheckedAt: string | null;
  desktopSettings: DesktopSettings;
  defaultApps: DefaultAppsStatus;
  firstRun: FirstRunState;
  /** Host calls, in order (`check`, `download`, `install`, `open-settings`). */
  calls: string[];
}

export function createSystemMockState(): SystemMockState {
  return {
    desktop: true,
    installation: {
      version: "1.0.0",
      kind: "nsis",
      scope: "per-user",
      architecture: "x64",
      install_dir: "C:\\Users\\ada\\AppData\\Local\\Programs\\Nail",
      updates_disabled: null,
      forced_channel: null,
      mapi_installed: true,
      webview_version: "131.0.2903.70",
    },
    updateSettings: { channel: "stable", auto_check: true, auto_download: true },
    updateState: { state: "idle" },
    lastCheckedAt: null,
    desktopSettings: {
      close_to_tray: true,
      start_hidden: true,
      start_with_windows: false,
      plugin_dev_endpoint: false,
    },
    defaultApps: {
      registered: true,
      associations: DEFAULT_APP_TARGETS.map((target) => ({
        target,
        registered: true,
        state: target === "mailto" ? "other" : "unknown",
      })),
      mapi: { registered: true, state: "other" },
    },
    firstRun: { completed: false, completed_at: null, completed_version: null },
    calls: [],
  };
}

/** The composed status (`UpdateStatus::compose`): the policy wins. */
export function updateStatusOf(state: SystemMockState): UpdateStatus {
  const { installation } = state;
  return {
    current_version: installation.version,
    settings: {
      ...state.updateSettings,
      channel: installation.forced_channel ?? state.updateSettings.channel,
    },
    forced_channel: installation.forced_channel,
    state:
      installation.updates_disabled === null
        ? structuredClone(state.updateState)
        : { state: "disabled", reason: installation.updates_disabled },
    last_checked_at: state.lastCheckedAt,
  };
}

/** Moves the simulated updater and emits `UpdateStatusChanged`. */
export function setUpdateState(core: MockCore, update: UpdateState): void {
  const state = core.data.features.system;
  state.updateState = update;
  if (update.state === "up-to-date" || update.state === "available") {
    state.lastCheckedAt = new Date().toISOString();
  }
  core.emit({ type: "UpdateStatusChanged", status: updateStatusOf(state) });
}

function error(
  code: "unsupported" | "forbidden" | "conflict",
  message: string,
  id?: string,
): Response {
  return {
    type: "Error",
    code,
    message,
    ...(id === undefined ? {} : { localized: { id, args: {} } }),
  };
}

function settingsUri(installation: InstallationInfo): string {
  switch (installation.scope) {
    case "per-user":
      return "ms-settings:defaultapps?registeredAppUser=Nail";
    case "per-machine":
      return "ms-settings:defaultapps?registeredAppMachine=Nail";
    default:
      return "ms-settings:defaultapps";
  }
}

export function answerSystem(core: MockCore, request: SystemRequest): Response {
  const state = core.data.features.system;
  if (request.type === "GetFirstRun") {
    return { type: "FirstRun", state: structuredClone(state.firstRun) };
  }
  if (request.type === "CompleteFirstRun") {
    state.firstRun = {
      completed: true,
      completed_at: new Date().toISOString(),
      completed_version: state.installation.version,
    };
    return { type: "FirstRun", state: structuredClone(state.firstRun) };
  }
  if (!state.desktop) {
    return error("unsupported", "only the desktop app has this", "desktop-unavailable");
  }
  const status = (): Response => ({ type: "UpdateStatus", status: updateStatusOf(state) });
  const desktop = (): Response => ({
    type: "DesktopSettings",
    settings: structuredClone(state.desktopSettings),
    dev_endpoint_url: state.desktopSettings.plugin_dev_endpoint ? "http://127.0.0.1:47811/" : null,
  });
  switch (request.type) {
    case "GetInstallation":
      return { type: "Installation", info: structuredClone(state.installation) };
    case "GetUpdateStatus":
      return status();
    case "SetUpdateSettings": {
      const forced = state.installation.forced_channel;
      if (forced !== null && forced !== request.settings.channel) {
        return error("forbidden", "the channel is set by policy", "update-channel-locked");
      }
      state.updateSettings = structuredClone(request.settings);
      core.emit({ type: "UpdateStatusChanged", status: updateStatusOf(state) });
      return status();
    }
    case "CheckForUpdates":
    case "DownloadUpdate":
    case "InstallUpdate": {
      const reason = state.installation.updates_disabled;
      if (reason !== null) {
        return error("forbidden", "updates are disabled", `update-disabled-${reason}`);
      }
      const allowed =
        request.type === "CheckForUpdates"
          ? canCheck(state.updateState)
          : request.type === "DownloadUpdate"
            ? canDownload(state.updateState)
            : canInstall(state.updateState);
      if (!allowed) return error("conflict", `${request.type} does not apply now`);
      if (request.type === "CheckForUpdates") {
        state.calls.push("check");
        state.updateState = { state: "checking" };
        return status();
      }
      if (request.type === "DownloadUpdate") {
        state.calls.push("download");
        return status();
      }
      state.calls.push("install");
      return { type: "Done" };
    }
    case "GetDesktopSettings":
      return desktop();
    case "SetDesktopSettings":
      state.desktopSettings = structuredClone(request.settings);
      core.emit({ type: "DesktopSettingsChanged" });
      return desktop();
    case "GetDefaultApps":
      return {
        type: "DefaultApps",
        status: structuredClone(state.defaultApps),
        settings_uri: settingsUri(state.installation),
      };
    case "OpenDefaultAppsSettings":
      state.calls.push("open-settings");
      return { type: "Done" };
  }
}
