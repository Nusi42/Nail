/**
 * The mock core's answers to the M2b remote-UI and remote-content requests
 * (ADR 0036), used by `MockTransport` (`../../api/mock.ts` routes exactly
 * these requests here). State lives in `data.features.remote`.
 *
 * Like the core after WP40: settings (validated, internet mode refused),
 * users, devices, sessions, pairing codes, the audit log and the
 * remote-content allow-lists work; password, TOTP, pairing-code creation
 * and certificate requests answer `unsupported` until WP41 / WP45. WP43
 * extends this mock for its UI.
 */
import type {
  PairingCode,
  RemoteAuditEntry,
  RemoteContentAllow,
  RemoteContentScope,
  RemoteDevice,
  RemoteGrant,
  RemoteSession,
  RemoteUiSettings,
  RemoteUiStatus,
  RemoteUser,
  Request,
  Response,
} from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";

/** The requests this feature answers. */
export const REMOTE_REQUESTS = [
  "GetRemoteUiSettings",
  "UpdateRemoteUiSettings",
  "RegenerateRemoteCertificate",
  "ListRemoteUsers",
  "CreateRemoteUser",
  "UpdateRemoteUser",
  "DeleteRemoteUser",
  "SetRemotePassword",
  "StartTotpEnrollment",
  "ConfirmTotpEnrollment",
  "DisableTotp",
  "UnlockRemoteUser",
  "RenamePasskey",
  "DeletePasskey",
  "ListRemoteDevices",
  "UpdateRemoteDevice",
  "RevokeRemoteDevice",
  "ListRemoteSessions",
  "RevokeRemoteSession",
  "RevokeAllRemoteSessions",
  "CreatePairingCode",
  "ListPairingCodes",
  "CancelPairingCode",
  "ListRemoteAudit",
  "AllowRemoteContent",
  "ListRemoteContentAllows",
  "RemoveRemoteContentAllow",
] as const satisfies readonly Request["type"][];

export type RemoteRequest = Extract<Request, { type: (typeof REMOTE_REQUESTS)[number] }>;

export function isRemoteRequest(request: Request): request is RemoteRequest {
  return (REMOTE_REQUESTS as readonly string[]).includes(request.type);
}

/** Mock-core state of this feature. */
export interface RemoteMockState {
  settings: RemoteUiSettings;
  users: RemoteUser[];
  devices: RemoteDevice[];
  sessions: RemoteSession[];
  pairing: PairingCode[];
  audit: RemoteAuditEntry[];
  allows: RemoteContentAllow[];
}

/** Defaults as the core has them (`RemoteUiSettings::default`). */
export function defaultRemoteUiSettings(): RemoteUiSettings {
  return {
    enabled: false,
    bind_address: "127.0.0.1",
    port: 4780,
    host_name: null,
    tls: "self-signed",
    acme: {
      domains: [],
      contact_email: null,
      directory_url: "https://acme-v02.api.letsencrypt.org/directory",
      accept_terms: false,
    },
    internet_mode: false,
    allowed_networks: [],
    session_idle_minutes: 30,
    session_max_hours: 12,
    reauth_minutes: 5,
  };
}

export function createRemoteMockState(): RemoteMockState {
  return {
    settings: defaultRemoteUiSettings(),
    users: [],
    devices: [],
    sessions: [],
    pairing: [],
    audit: [],
    allows: [],
  };
}

/** Not running until WP45; internet mode locked (the pentest gate). */
export function stoppedStatus(activeSessions = 0): RemoteUiStatus {
  return {
    state: "stopped",
    urls: [],
    certificate: null,
    internet_mode_locked: true,
    internet_mode_lock_reason: "pentest-pending",
    active_sessions: activeSessions,
    error: null,
    message: null,
  };
}

function state(core: MockCore): RemoteMockState {
  return core.data.features.remote;
}

function now(): string {
  return new Date().toISOString().replace(/\.\d{3}Z$/, "Z");
}

function isLoopback(address: string): boolean {
  return address === "::1" || /^127\.\d{1,3}\.\d{1,3}\.\d{1,3}$/.test(address);
}

function isIp(address: string): boolean {
  return (
    /^\d{1,3}(\.\d{1,3}){3}$/.test(address) || (address.includes(":") && !address.includes("."))
  );
}

/** The core's checks (`RemoteUiSettings::issues`), most important first. */
function settingsError(settings: RemoteUiSettings): Response | null {
  if (settings.internet_mode || settings.tls === "acme") {
    return errorResponse("forbidden", "internet mode is locked (pentest-pending)");
  }
  if (!isIp(settings.bind_address)) {
    return errorResponse("invalid_request", "invalid-bind-address");
  }
  if (!isLoopback(settings.bind_address) && settings.tls === "off") {
    return errorResponse("invalid_request", "tls-required");
  }
  if (settings.port === 0) return errorResponse("invalid_request", "invalid-port");
  return null;
}

/** Intersection of two grants (`RemoteGrant::intersect`). */
export function intersectGrants(a: RemoteGrant, b: RemoteGrant): RemoteGrant {
  const accounts =
    a.mail.all_accounts && b.mail.all_accounts
      ? []
      : a.mail.all_accounts
        ? b.mail.accounts
        : b.mail.all_accounts
          ? a.mail.accounts
          : a.mail.accounts.filter((x) => b.mail.accounts.includes(x));
  return {
    mail: {
      scopes: a.mail.scopes.filter((s) => b.mail.scopes.includes(s)),
      all_accounts: a.mail.all_accounts && b.mail.all_accounts,
      accounts: [...accounts],
      excluded_folders: [...new Set([...a.mail.excluded_folders, ...b.mail.excluded_folders])],
      skip_draft_confirmation: false,
    },
    admin: a.admin && b.admin,
  };
}

function normalizeSender(address: string): string | null {
  const at = address.lastIndexOf("@");
  if (at <= 0 || at === address.length - 1) return null;
  return address.trim().toLowerCase();
}

function allowValue(core: MockCore, messageId: string, scope: RemoteContentScope) {
  const message = core.data.messages.find((m) => m.id === messageId);
  if (message === undefined) return { error: errorResponse("not_found", `message ${messageId}`) };
  if (scope === "message") return { value: messageId, label: message.subject };
  const sender = message.from === null ? null : normalizeSender(message.from.email);
  if (sender === null) {
    return { error: errorResponse("invalid_request", `message ${messageId} has no usable sender`) };
  }
  return {
    value: scope === "sender" ? sender : sender.slice(sender.lastIndexOf("@") + 1),
    label: null,
  };
}

function endSessions(
  core: MockCore,
  match: (s: RemoteSession) => boolean,
  reason: RemoteSession["end_reason"],
) {
  const at = now();
  for (const session of state(core).sessions) {
    if (session.ended_at === null && match(session)) {
      session.ended_at = at;
      session.end_reason = reason;
      if (reason !== null) {
        core.emit({ type: "RemoteSessionEnded", session_id: session.id, reason });
      }
    }
  }
}

export function answerRemote(core: MockCore, request: RemoteRequest): Response {
  const s = state(core);
  const active = () => s.sessions.filter((x) => x.ended_at === null).length;
  const changed = () => core.emit({ type: "RemoteAccessChanged" });
  const user = (id: string) => s.users.find((u) => u.id === id);
  switch (request.type) {
    case "GetRemoteUiSettings":
      return {
        type: "RemoteUi",
        settings: structuredClone(s.settings),
        status: stoppedStatus(active()),
      };
    case "UpdateRemoteUiSettings": {
      const error = settingsError(request.settings);
      if (error !== null) return error;
      s.settings = structuredClone(request.settings);
      const status = stoppedStatus(active());
      core.emit({ type: "RemoteUiStatusChanged", status });
      return { type: "RemoteUi", settings: structuredClone(s.settings), status };
    }
    case "ListRemoteUsers":
      return { type: "RemoteUsers", users: structuredClone(s.users) };
    case "CreateRemoteUser": {
      const name = request.name.trim();
      if (name === "") return errorResponse("invalid_request", "a remote user needs a name");
      if (s.users.some((u) => u.name.toLowerCase() === name.toLowerCase())) {
        return errorResponse("conflict", `a remote user named ${name} exists`);
      }
      const created: RemoteUser = {
        id: core.nextId("ruser-"),
        name,
        display_name: request.display_name,
        created_at: now(),
        last_login_at: null,
        has_password: false,
        totp_enabled: false,
        passkeys: [],
        grant: structuredClone(request.grant),
        disabled: false,
        locked_until: null,
      };
      s.users.push(created);
      changed();
      return { type: "RemoteUserSaved", user: structuredClone(created) };
    }
    case "UpdateRemoteUser": {
      const found = user(request.user_id);
      if (found === undefined) return errorResponse("not_found", `remote user ${request.user_id}`);
      const { display_name, grant, disabled } = request.update;
      if (display_name !== null)
        found.display_name = display_name.trim() === "" ? null : display_name.trim();
      if (grant !== null) {
        found.grant = structuredClone(grant);
        for (const device of s.devices.filter((d) => d.user_id === found.id)) {
          device.grant = intersectGrants(device.grant, grant);
        }
      }
      if (disabled !== null) {
        found.disabled = disabled;
        if (disabled) endSessions(core, (x) => x.user_id === found.id, "user-changed");
      }
      changed();
      return { type: "RemoteUserSaved", user: structuredClone(found) };
    }
    case "DeleteRemoteUser": {
      if (user(request.user_id) === undefined) {
        return errorResponse("not_found", `remote user ${request.user_id}`);
      }
      endSessions(core, (x) => x.user_id === request.user_id, "user-changed");
      s.users = s.users.filter((u) => u.id !== request.user_id);
      s.devices = s.devices.filter((d) => d.user_id !== request.user_id);
      s.sessions = s.sessions.filter((x) => x.user_id !== request.user_id);
      s.pairing = s.pairing.filter((p) => p.user_id !== request.user_id);
      changed();
      return { type: "Done" };
    }
    case "UnlockRemoteUser": {
      const found = user(request.user_id);
      if (found === undefined) return errorResponse("not_found", `remote user ${request.user_id}`);
      found.locked_until = null;
      changed();
      return { type: "RemoteUserSaved", user: structuredClone(found) };
    }
    case "RenamePasskey":
    case "DeletePasskey": {
      const owner = s.users.find((u) => u.passkeys.some((p) => p.id === request.passkey_id));
      if (owner === undefined) return errorResponse("not_found", `passkey ${request.passkey_id}`);
      if (request.type === "RenamePasskey") {
        if (request.name.trim() === "")
          return errorResponse("invalid_request", "a passkey needs a name");
        const key = owner.passkeys.find((p) => p.id === request.passkey_id);
        if (key !== undefined) key.name = request.name.trim();
      } else {
        owner.passkeys = owner.passkeys.filter((p) => p.id !== request.passkey_id);
      }
      changed();
      return { type: "RemoteUserSaved", user: structuredClone(owner) };
    }
    case "ListRemoteDevices":
      return {
        type: "RemoteDevices",
        devices: structuredClone(
          s.devices.filter((d) => request.user_id === null || d.user_id === request.user_id),
        ),
      };
    case "UpdateRemoteDevice": {
      const device = s.devices.find((d) => d.id === request.device_id);
      if (device === undefined)
        return errorResponse("not_found", `remote device ${request.device_id}`);
      if (request.name !== null && request.name.trim() !== "") device.name = request.name.trim();
      const owner = user(device.user_id);
      if (request.grant !== null && owner !== undefined) {
        device.grant = intersectGrants(request.grant, owner.grant);
      }
      changed();
      return { type: "RemoteDeviceSaved", device: structuredClone(device) };
    }
    case "RevokeRemoteDevice": {
      const device = s.devices.find((d) => d.id === request.device_id);
      if (device === undefined)
        return errorResponse("not_found", `remote device ${request.device_id}`);
      device.revoked_at ??= now();
      endSessions(core, (x) => x.device_id === device.id, "device-revoked");
      changed();
      return { type: "Done" };
    }
    case "ListRemoteSessions": {
      const { user_id, device_id, include_ended } = request.filter;
      const sessions = s.sessions.filter(
        (x) =>
          (user_id === null || x.user_id === user_id) &&
          (device_id === null || x.device_id === device_id) &&
          (include_ended || x.ended_at === null),
      );
      return { type: "RemoteSessions", sessions: structuredClone(sessions).reverse() };
    }
    case "RevokeRemoteSession":
      endSessions(core, (x) => x.id === request.session_id, "revoked");
      changed();
      return { type: "Done" };
    case "RevokeAllRemoteSessions":
      endSessions(
        core,
        (x) => request.user_id === null || x.user_id === request.user_id,
        "revoked",
      );
      changed();
      return { type: "Done" };
    case "ListPairingCodes":
      return { type: "PairingCodes", codes: structuredClone(s.pairing) };
    case "CancelPairingCode": {
      const code = s.pairing.find((p) => p.id === request.pairing_id);
      if (code === undefined)
        return errorResponse("not_found", `pairing code ${request.pairing_id}`);
      if (code.state === "pending") {
        code.state = "cancelled";
        changed();
      }
      return { type: "Done" };
    }
    case "ListRemoteAudit": {
      const entries = s.audit
        .filter((e) => request.user_id === null || e.user_id === request.user_id)
        .filter((e) => request.before === null || e.id < request.before)
        .sort((a, b) => b.id - a.id)
        .slice(0, request.limit);
      return { type: "RemoteAudit", entries: structuredClone(entries) };
    }
    case "AllowRemoteContent": {
      const found = allowValue(core, request.message_id, request.scope);
      if ("error" in found) return found.error;
      const entry: RemoteContentAllow = {
        scope: request.scope,
        value: found.value,
        label: found.label,
        created_at: now(),
      };
      s.allows = s.allows.filter((a) => !(a.scope === entry.scope && a.value === entry.value));
      s.allows.push(entry);
      core.emit({ type: "RemoteContentPolicyChanged" });
      return { type: "RemoteContentAllowed", entry: structuredClone(entry) };
    }
    case "ListRemoteContentAllows": {
      const order: RemoteContentScope[] = ["message", "sender", "domain"];
      const entries = [...s.allows].sort(
        (a, b) =>
          order.indexOf(a.scope) - order.indexOf(b.scope) ||
          b.created_at.localeCompare(a.created_at),
      );
      return { type: "RemoteContentAllows", entries: structuredClone(entries) };
    }
    case "RemoveRemoteContentAllow": {
      const value = request.value.trim().toLowerCase();
      const before = s.allows.length;
      s.allows = s.allows.filter(
        (a) => !(a.scope === request.scope && a.value.toLowerCase() === value),
      );
      if (s.allows.length !== before) core.emit({ type: "RemoteContentPolicyChanged" });
      return { type: "Done" };
    }
    case "RegenerateRemoteCertificate":
    case "SetRemotePassword":
    case "StartTotpEnrollment":
    case "ConfirmTotpEnrollment":
    case "DisableTotp":
    case "CreatePairingCode":
      return errorResponse("unsupported", `${request.type} is not available yet (remote UI, M2b)`);
  }
}
