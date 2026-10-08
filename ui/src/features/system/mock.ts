/**
 * The mock core's answers to the M4b desktop requests (ADR 0087), used by
 * `MockTransport` (`../../api/mock.ts` routes exactly these requests here).
 *
 * Like the core with a desktop host: settings are stored in
 * `data.features.system`; the host's side (installation facts, updater,
 * update server, Run key, default apps) is simulated there too. The policy
 * wins (a disabled updater reports `disabled` and refuses checks with
 * `forbidden`; a forced channel cannot be changed), checks / downloads /
 * installs follow the state machine (`conflict` otherwise).
 *
 * The simulated updater moves on by itself like the shell's: a check finds
 * the newest release of the channel on the simulated update server
 * (`releases`), downloads it in steps (automatically with `auto_download`)
 * and becomes "ready to install"; `failNext` makes the next check or
 * download fail. Tests and demos also move it by hand with
 * `setUpdateState`, which cancels the steps under way. With
 * `desktop: false` everything but the first run answers `unsupported` like
 * `nail serve`.
 *
 * `simulateMapiSendMail` plays the Simple MAPI path (`Service::submit_mapi`):
 * a program's "Send to" becomes a saved draft plus a `ComposeDraft` launch
 * request — never a sent message.
 */
import {
  UPDATE_DISABLED_REASON_MESSAGE_IDS,
  type Address,
  type DefaultAppsStatus,
  type DesktopSettings,
  type Draft,
  type FirstRunState,
  type InstallationInfo,
  type MapiResult,
  type Request,
  type Response,
  type UpdateChannel,
  type UpdateFailure,
  type UpdateInfo,
  type UpdateSettings,
  type UpdateState,
  type UpdateStatus,
} from "../../api/generated";
import type { MockCore } from "../../api/mockMail";
import { answerComposer, saveRichDraft } from "../composer/mock";
import { DEFAULT_APP_TARGETS, canCheck, canDownload, canInstall, compareVersions } from "./model";

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
  /** The machine-wide Run key (MSI `AUTOSTART=1`) starts Nail for everyone. */
  startWithWindowsManaged: boolean;
  defaultApps: DefaultAppsStatus;
  firstRun: FirstRunState;
  /** Host calls, in order (`check`, `download`, `install`, `open-settings`). */
  calls: string[];
  /** The newest release per channel on the simulated update server. */
  releases: Partial<Record<UpdateChannel, UpdateInfo>>;
  /** The next check / download fails this way (then it is cleared). */
  failNext: UpdateFailure | null;
  /** Whether checks and downloads move on by themselves (off = by hand). */
  autoProgress: boolean;
  /** Download steps until "ready to install". */
  downloadSteps: number;
  /** Bumped by every state change; pending steps of an older one stop. */
  generation: number;
}

const RELEASE_NOTES = `## What's new

- Faster search in large folders
- **Calendar:** week numbers in the month view
- Fixes for [Exchange](https://example.org/notes) shared mailboxes`;

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
    startWithWindowsManaged: false,
    defaultApps: {
      registered: true,
      associations: DEFAULT_APP_TARGETS.map((target) => ({
        target,
        registered: true,
        state: target === "mailto" ? "other" : "unknown",
      })),
      mapi: { registered: true, state: "other" },
    },
    // A returning user: the first-run screen would cover every mock view.
    // Demos and tests set `completed: false` to see it.
    firstRun: {
      completed: true,
      completed_at: "2026-09-01T08:00:00Z",
      completed_version: "1.0.0",
    },
    calls: [],
    releases: {
      stable: {
        version: "1.1.0",
        channel: "stable",
        published_at: "2026-09-30T09:00:00Z",
        notes: RELEASE_NOTES,
        size_bytes: 48_000_000,
      },
      beta: {
        version: "1.2.0-beta.1",
        channel: "beta",
        published_at: "2026-10-01T09:00:00Z",
        notes: "- Try the new command palette",
        size_bytes: 49_000_000,
      },
    },
    failNext: null,
    autoProgress: true,
    downloadSteps: 4,
    generation: 0,
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

/** Sets the updater state and emits `UpdateStatusChanged` (no generation bump). */
function move(core: MockCore, update: UpdateState): void {
  const state = core.data.features.system;
  state.updateState = update;
  if (update.state === "up-to-date" || update.state === "available") {
    state.lastCheckedAt = new Date().toISOString();
  }
  core.emit({ type: "UpdateStatusChanged", status: updateStatusOf(state) });
}

/** Moves the simulated updater by hand and emits `UpdateStatusChanged`. */
export function setUpdateState(core: MockCore, update: UpdateState): void {
  core.data.features.system.generation++;
  move(core, update);
}

/** Runs `step` later unless the updater changed meanwhile. */
function later(core: MockCore, step: () => void): void {
  const state = core.data.features.system;
  if (!state.autoProgress) return;
  const generation = state.generation;
  core.later(() => {
    if (state.generation === generation) step();
  });
}

/** The newest release for the effective channel, when newer than this one. */
function newerRelease(state: SystemMockState): UpdateInfo | null {
  const channel = state.installation.forced_channel ?? state.updateSettings.channel;
  const candidates = [state.releases.stable, channel === "beta" ? state.releases.beta : undefined];
  let best: UpdateInfo | null = null;
  for (const release of candidates) {
    if (release === undefined) continue;
    if (compareVersions(release.version, state.installation.version) <= 0) continue;
    if (best === null || compareVersions(release.version, best.version) > 0) best = release;
  }
  return best === null ? null : structuredClone(best);
}

function takeFailure(state: SystemMockState): UpdateFailure | null {
  const failure = state.failNext;
  state.failNext = null;
  return failure;
}

function runCheck(core: MockCore): void {
  later(core, () => {
    const state = core.data.features.system;
    const failure = takeFailure(state);
    if (failure !== null) {
      move(core, { state: "failed", failure, detail: "mock: check failed", update: null });
      return;
    }
    const update = newerRelease(state);
    if (update === null) {
      move(core, { state: "up-to-date" });
      return;
    }
    move(core, { state: "available", update });
    if (state.updateSettings.auto_download) startDownload(core, update);
  });
}

function startDownload(core: MockCore, update: UpdateInfo): void {
  const state = core.data.features.system;
  state.generation++;
  const total = update.size_bytes;
  move(core, { state: "downloading", update, downloaded_bytes: 0, total_bytes: total });
  const steps = Math.max(1, state.downloadSteps);
  const step = (n: number) => {
    later(core, () => {
      if (n === 1) {
        const failure = takeFailure(state);
        if (failure !== null) {
          move(core, { state: "failed", failure, detail: "mock: download failed", update });
          return;
        }
      }
      if (n >= steps) {
        move(core, { state: "ready-to-install", update });
        return;
      }
      const done = total === null ? n * 1_000_000 : Math.round((total * n) / steps);
      move(core, { state: "downloading", update, downloaded_bytes: done, total_bytes: total });
      step(n + 1);
    });
  };
  step(1);
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
    settings: {
      ...structuredClone(state.desktopSettings),
      // As the shell: the machine's Run key keeps it on.
      start_with_windows: state.desktopSettings.start_with_windows || state.startWithWindowsManaged,
    },
    dev_endpoint_url: state.desktopSettings.plugin_dev_endpoint ? "http://127.0.0.1:47811/" : null,
    start_with_windows_managed: state.startWithWindowsManaged,
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
        return error(
          "forbidden",
          "updates are disabled",
          UPDATE_DISABLED_REASON_MESSAGE_IDS[reason],
        );
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
        state.generation++;
        move(core, { state: "checking" });
        runCheck(core);
        return status();
      }
      if (request.type === "DownloadUpdate") {
        state.calls.push("download");
        if (state.updateState.state === "available") startDownload(core, state.updateState.update);
        return status();
      }
      state.calls.push("install");
      if (state.updateState.state === "ready-to-install") {
        state.generation++;
        move(core, { state: "installing", update: state.updateState.update });
      }
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

/** Like the user choosing Nail in Settings → Default apps (emits `DefaultAppsChanged`). */
export function setDefaultApps(core: MockCore, change: (status: DefaultAppsStatus) => void): void {
  change(core.data.features.system.defaultApps);
  core.emit({ type: "DefaultAppsChanged" });
}

// ---- Simple MAPI ("Send to" from another program) ----

/** A recipient as `MAPISendMail` passes it (`nail_api::mapi::MapiRecipient`). */
export interface MockMapiRecipient {
  class: "to" | "cc" | "bcc";
  name: string | null;
  address: string | null;
}

/**
 * A file the program attaches; `data` (base64) and `mime` stand in for the
 * file on disk — no `data` = the file was not found.
 */
export interface MockMapiAttachment {
  path: string;
  file_name: string | null;
  mime?: string;
  data?: string;
}

/** One `MAPISendMail(W)` call (`nail_api::mapi::MapiMessage`). */
export interface MockMapiMessage {
  subject: string | null;
  body: string | null;
  recipients: readonly MockMapiRecipient[];
  attachments: readonly MockMapiAttachment[];
  ui: "dialog" | "modeless" | "none";
  program: string | null;
}

const MAPI_ADDRESS = /^[^\s@<>]+@[^\s@<>]+$/;

/** `MapiRecipient::email`: the address without `SMTP:` / `<>`, else a name that is one. */
export function mapiRecipientEmail(recipient: MockMapiRecipient): string | null {
  const strip = (text: string | null) => {
    if (text === null) return null;
    const value = text
      .trim()
      .replace(/^smtp:/i, "")
      .replace(/^<(.*)>$/, "$1")
      .trim();
    return MAPI_ADDRESS.test(value) ? value : null;
  };
  return strip(recipient.address) ?? strip(recipient.name);
}

function mapiFileName(attachment: MockMapiAttachment): string {
  const last = (text: string | null) => {
    const name = text?.split(/[\\/]/).pop()?.trim() ?? "";
    return name === "" ? null : name;
  };
  return last(attachment.file_name) ?? last(attachment.path) ?? "attachment";
}

/**
 * Plays `Service::submit_mapi`: checks the message like `MapiMessage::check`,
 * reads the files, saves a plain-text draft from the default identity (the
 * body above the signature) and queues `LaunchRequest::ComposeDraft`.
 * Answers the MAPI result the DLL would return; nothing is ever sent, even
 * for `ui: "none"`.
 */
export function simulateMapiSendMail(core: MockCore, message: MockMapiMessage): MapiResult {
  if (message.recipients.length > 500) return "too-many-recipients";
  if (message.attachments.length > 100) return "too-many-files";
  if ((message.subject?.length ?? 0) + (message.body?.length ?? 0) > 8 * 1024 * 1024) {
    return "text-too-large";
  }
  const emails = message.recipients.map(mapiRecipientEmail);
  if (emails.some((e) => e === null)) return "invalid-recips";
  if (message.attachments.some((a) => !/^(?:[A-Za-z]:\\|\\\\)/.test(a.path))) {
    return "attachment-not-found";
  }
  if (message.attachments.some((a) => a.data === undefined)) return "attachment-not-found";

  const created = answerComposer(core, {
    type: "NewDraft",
    identity_id: null,
    account_id: null,
    mode: "plain",
    context: null,
  });
  if (created.type !== "Draft") return "login-failure";
  const draft: Draft = created.draft;
  const byClass = (cls: MockMapiRecipient["class"]): Address[] =>
    message.recipients.flatMap((r, i) =>
      r.class === cls
        ? [{ name: r.name !== emails[i] ? r.name : null, email: emails[i] ?? "" }]
        : [],
    );
  draft.to = byClass("to");
  draft.cc = byClass("cc");
  draft.bcc = byClass("bcc");
  draft.subject = message.subject ?? "";
  // The program's text above the signature the new draft carries.
  draft.text = `${message.body ?? ""}${draft.text}`;
  for (const attachment of message.attachments) {
    const answer = answerComposer(core, {
      type: "UploadAttachment",
      filename: mapiFileName(attachment),
      mime: attachment.mime ?? "application/octet-stream",
      data: attachment.data ?? "",
      content_id: null,
    });
    if (answer.type !== "AttachmentUploaded") {
      return answer.type === "Error" && answer.code === "too_large"
        ? "attachment-too-large"
        : "attachment-open-failure";
    }
    draft.attachments.push(answer.attachment);
  }
  const saved = saveRichDraft(core, draft);
  if (saved.type !== "DraftSaved") return "failure";
  core.data.launches.push({
    type: "ComposeDraft",
    draft_id: saved.draft_id,
    program: message.program,
  });
  core.emit({ type: "LaunchRequested" });
  return "success";
}
