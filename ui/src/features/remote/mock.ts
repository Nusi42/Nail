/**
 * The mock core's answers to the M2b remote-UI and remote-content requests
 * (ADR 0036), used by `MockTransport` (`../../api/mock.ts` routes exactly
 * these requests here). State lives in `data.features.remote`.
 *
 * WP43 made it behave like the core after WP45 so the UI can be built and
 * tested against it: settings (validated, internet mode refused; a mock
 * status that is `running` while enabled), users, passwords, TOTP
 * enrollment, pairing codes, devices, sessions, the audit log, the
 * certificate and the remote-content allow-lists. The browser side of the
 * login protocol (`POST /api/auth`) is `./mockAuth.ts`, on the same state.
 *
 * Secrets are kept in plain text in `secrets` — this is a test double; the
 * real store keeps only Argon2id hashes, `keychain:` references and
 * SHA-256 hashes.
 */
import type {
  MessageBody,
  PairingCode,
  RemoteAuditEntry,
  RemoteCertificateInfo,
  RemoteContentAllow,
  RemoteContentInfo,
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
import { normalizePairingCode, settingsIssues } from "./model";

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

/** The TOTP code the mock accepts (it does not run RFC 6238). */
export const MOCK_TOTP_CODE = "123456";
/** Failed logins before the mock locks a user or an address out. */
export const MOCK_MAX_FAILURES = 5;
/** Lockout length of the mock, in seconds. */
export const MOCK_LOCKOUT_SECS = 300;
/** Shortest password the mock accepts. */
export const MOCK_MIN_PASSWORD = 12;

/** Test-double secrets (see the module docs). */
export interface RemoteMockSecrets {
  /** User id → password. */
  passwords: Record<string, string>;
  /** User id → TOTP secret, `confirmed` once enrolled. */
  totp: Record<string, { secret: string; confirmed: boolean }>;
  /** Pairing id → the code. */
  pairingCodes: Record<string, string>;
  /** WebAuthn credential id → passkey id. */
  credentials: Record<string, string>;
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
  /** The served certificate (self-signed), regenerated on request. */
  certificate: RemoteCertificateInfo;
  secrets: RemoteMockSecrets;
  /** Failed login attempts per key (`ip:<addr>` / `user:<name>`). */
  failures: Record<string, number>;
  /** Locked-out addresses → until (RFC 3339). */
  lockedIps: Record<string, string>;
  /** Remote resources per message id (default: 3 for blocked messages). */
  resources: Record<string, number>;
  /** Behaviour switches: requests answered `unsupported` (as before WP45). */
  unsupported: RemoteRequest["type"][];
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

const HEX = "0123456789ABCDEF";
/** Pairing-code alphabet (`nail_remote_auth::pairing::ALPHABET`): no 0/O, 1/I/L, U/V. */
const CODE_ALPHABET = "ABCDEFGHJKMNPQRSTWXYZ23456789";
const BASE32 = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

function randomFrom(alphabet: string, length: number): string {
  const bytes = new Uint8Array(length);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => alphabet[b % alphabet.length]).join("");
}

/** A fresh self-signed certificate record (random fingerprint). */
export function mockCertificate(
  names: readonly string[] = ["nail-desktop.local"],
): RemoteCertificateInfo {
  const from = new Date();
  const until = new Date(from.getTime() + 365 * 24 * 3600 * 1000);
  return {
    source: "self-signed",
    fingerprint_sha256: randomFrom(HEX, 64),
    subject_names: [...names],
    not_before: stamp(from),
    not_after: stamp(until),
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
    certificate: mockCertificate(),
    secrets: { passwords: {}, totp: {}, pairingCodes: {}, credentials: {} },
    failures: {},
    lockedIps: {},
    resources: {},
    unsupported: [],
  };
}

/** Not running; internet mode locked (the pentest gate). */
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

/** The mock's status: `running` while enabled, with URLs and certificate. */
export function mockStatus(s: RemoteMockState): RemoteUiStatus {
  const active = s.sessions.filter((x) => x.ended_at === null).length;
  if (!s.settings.enabled) return stoppedStatus(active);
  const scheme = s.settings.tls === "off" ? "http" : "https";
  const port = s.settings.port;
  const hosts =
    s.settings.bind_address === "0.0.0.0" || s.settings.bind_address === "::"
      ? [s.settings.host_name ?? "nail-desktop.local", "192.168.1.20"]
      : [s.settings.host_name ?? s.settings.bind_address];
  const urls = [...new Set(hosts)].map(
    (h) => `${scheme}://${h.includes(":") ? `[${h}]` : h}:${port}/`,
  );
  return {
    ...stoppedStatus(active),
    state: "running",
    urls,
    certificate: s.settings.tls === "off" ? null : structuredClone(s.certificate),
  };
}

export function remoteState(core: MockCore): RemoteMockState {
  return core.data.features.remote;
}

/** Removes `key` from a record (the mock's maps are plain objects). */
export function forget(record: Record<string, unknown>, key: string): void {
  Reflect.deleteProperty(record, key);
}

function stamp(date: Date): string {
  return date.toISOString().replace(/\.\d{3}Z$/, "Z");
}

export function now(): string {
  return stamp(new Date());
}

export function later(seconds: number): string {
  return stamp(new Date(Date.now() + seconds * 1000));
}

/** The core's checks (`RemoteUiSettings::issues`), most important first. */
function settingsError(settings: RemoteUiSettings): Response | null {
  const issues = settingsIssues(settings);
  const forbidden = issues.find(
    (i) => i === "internet-mode-locked" || i === "acme-needs-internet-mode",
  );
  if (forbidden !== undefined) {
    return errorResponse("forbidden", `${forbidden}: internet mode is locked (pentest-pending)`);
  }
  const first = issues[0];
  return first === undefined ? null : errorResponse("invalid_request", first);
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

function senderOf(body: MessageBody): string | null {
  const from = body.headers.from[0];
  return from === undefined ? null : normalizeSender(from.email);
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

/**
 * The allow-list entry that lets `body`'s remote content load, most
 * specific first (`Store::remote_content_allowed_by`).
 */
export function remoteContentAllowedBy(
  s: RemoteMockState,
  messageId: string,
  sender: string | null,
): RemoteContentScope | null {
  const has = (scope: RemoteContentScope, value: string | null) =>
    value !== null && s.allows.some((a) => a.scope === scope && a.value === value);
  const domain = sender === null ? null : sender.slice(sender.lastIndexOf("@") + 1);
  if (has("message", messageId)) return "message";
  if (has("sender", sender)) return "sender";
  if (has("domain", domain)) return "domain";
  return null;
}

/**
 * What the core puts into `MessageBody.remote_content` (WP45): mock
 * messages that were flagged `remote_content_blocked` have remote
 * resources; an allow-list entry unblocks them. The integrator chains this
 * after the security decoration in `MockTransport`'s `GetMessage`.
 */
export function decorateRemoteContent(core: MockCore, body: MessageBody): MessageBody {
  const s = remoteState(core);
  const known = s.resources[body.id];
  const resources = known ?? (body.remote_content_blocked || body.remote_content ? 3 : 0);
  if (resources === 0) return body;
  const sender = senderOf(body);
  const allowedBy = remoteContentAllowedBy(s, body.id, sender);
  const info: RemoteContentInfo = {
    resources,
    allowed_by: allowedBy,
    sender,
    domain: sender === null ? null : sender.slice(sender.lastIndexOf("@") + 1),
  };
  return { ...body, remote_content: info, remote_content_blocked: allowedBy === null };
}

function endSessions(
  core: MockCore,
  match: (s: RemoteSession) => boolean,
  reason: RemoteSession["end_reason"],
) {
  const at = now();
  for (const session of remoteState(core).sessions) {
    if (session.ended_at === null && match(session)) {
      session.ended_at = at;
      session.end_reason = reason;
      if (reason !== null) {
        core.emit({ type: "RemoteSessionEnded", session_id: session.id, reason });
      }
    }
  }
}

/** Appends an audit entry (the core writes these in WP41 / WP45). */
export function writeAudit(
  core: MockCore,
  entry: Partial<Omit<RemoteAuditEntry, "id" | "at">> & Pick<RemoteAuditEntry, "kind" | "outcome">,
): RemoteAuditEntry {
  const s = remoteState(core);
  const full: RemoteAuditEntry = {
    id: (s.audit.reduce((max, e) => Math.max(max, e.id), 0) || 0) + 1,
    at: now(),
    user_id: null,
    user_name: null,
    device_id: null,
    session_id: null,
    ip: null,
    user_agent: null,
    method: null,
    name: null,
    error: null,
    detail: null,
    ...entry,
  };
  s.audit.push(full);
  return full;
}

/** Marks pending codes past their expiry as expired. */
export function expirePairingCodes(s: RemoteMockState, at: number = Date.now()): void {
  for (const code of s.pairing) {
    if (code.state === "pending" && new Date(code.expires_at).getTime() <= at) {
      code.state = "expired";
    }
  }
}

/** The pending, unexpired pairing code `typed` matches, if any. */
export function findPairingCode(s: RemoteMockState, typed: string): PairingCode | undefined {
  expirePairingCodes(s);
  const code = normalizePairingCode(typed);
  return s.pairing.find(
    (p) => p.state === "pending" && s.secrets.pairingCodes[p.id] === code && code !== "",
  );
}

export function answerRemote(core: MockCore, request: RemoteRequest): Response {
  const s = remoteState(core);
  if (s.unsupported.includes(request.type)) {
    return errorResponse("unsupported", `${request.type} is not available yet (remote UI, M2b)`);
  }
  const changed = () => core.emit({ type: "RemoteAccessChanged" });
  const user = (id: string) => s.users.find((u) => u.id === id);
  const notFoundUser = (id: string) => errorResponse("not_found", `remote user ${id}`);
  switch (request.type) {
    case "GetRemoteUiSettings":
      return { type: "RemoteUi", settings: structuredClone(s.settings), status: mockStatus(s) };
    case "UpdateRemoteUiSettings": {
      const error = settingsError(request.settings);
      if (error !== null) return error;
      s.settings = structuredClone(request.settings);
      const status = mockStatus(s);
      core.emit({ type: "RemoteUiStatusChanged", status });
      return { type: "RemoteUi", settings: structuredClone(s.settings), status };
    }
    case "RegenerateRemoteCertificate": {
      if (s.settings.tls !== "self-signed") {
        return errorResponse("invalid_request", "only a self-signed certificate is regenerated");
      }
      s.certificate = mockCertificate(s.certificate.subject_names);
      const status = mockStatus(s);
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
      if (found === undefined) return notFoundUser(request.user_id);
      const { display_name, grant, disabled } = request.update;
      if (display_name !== null)
        found.display_name = display_name.trim() === "" ? null : display_name.trim();
      if (grant !== null) {
        found.grant = structuredClone(grant);
        for (const device of s.devices.filter((d) => d.user_id === found.id)) {
          device.grant = intersectGrants(device.grant, grant);
        }
        for (const session of s.sessions.filter((x) => x.user_id === found.id)) {
          session.grant = intersectGrants(session.grant, grant);
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
      if (user(request.user_id) === undefined) return notFoundUser(request.user_id);
      endSessions(core, (x) => x.user_id === request.user_id, "user-changed");
      s.users = s.users.filter((u) => u.id !== request.user_id);
      s.devices = s.devices.filter((d) => d.user_id !== request.user_id);
      s.sessions = s.sessions.filter((x) => x.user_id !== request.user_id);
      s.pairing = s.pairing.filter((p) => p.user_id !== request.user_id);
      forget(s.secrets.passwords, request.user_id);
      forget(s.secrets.totp, request.user_id);
      changed();
      return { type: "Done" };
    }
    case "SetRemotePassword": {
      const found = user(request.user_id);
      if (found === undefined) return notFoundUser(request.user_id);
      if (request.password === null) {
        forget(s.secrets.passwords, found.id);
        found.has_password = false;
      } else {
        if ([...request.password].length < MOCK_MIN_PASSWORD) {
          return errorResponse("invalid_request", "password-too-short");
        }
        s.secrets.passwords[found.id] = request.password;
        found.has_password = true;
      }
      writeAudit(core, {
        kind: "password-changed",
        outcome: "success",
        user_id: found.id,
        user_name: found.name,
      });
      changed();
      return { type: "RemoteUserSaved", user: structuredClone(found) };
    }
    case "StartTotpEnrollment": {
      const found = user(request.user_id);
      if (found === undefined) return notFoundUser(request.user_id);
      const secret = randomFrom(BASE32, 32);
      s.secrets.totp[found.id] = {
        secret,
        confirmed: s.secrets.totp[found.id]?.confirmed ?? false,
      };
      const label = encodeURIComponent(`Nail:${found.name}`);
      return {
        type: "TotpEnrollment",
        enrollment: {
          user_id: found.id,
          secret,
          otpauth_uri: `otpauth://totp/${label}?secret=${secret}&issuer=Nail&algorithm=SHA1&digits=6&period=30`,
          issuer: "Nail",
          account_name: found.name,
          digits: 6,
          period_secs: 30,
        },
      };
    }
    case "ConfirmTotpEnrollment": {
      const found = user(request.user_id);
      if (found === undefined) return notFoundUser(request.user_id);
      const pending = s.secrets.totp[found.id];
      if (pending === undefined) return errorResponse("invalid_request", "no enrollment started");
      if (request.code.replace(/\s/g, "") !== MOCK_TOTP_CODE) {
        return errorResponse("auth_failed", "the code does not match");
      }
      pending.confirmed = true;
      found.totp_enabled = true;
      writeAudit(core, {
        kind: "totp-changed",
        outcome: "success",
        user_id: found.id,
        user_name: found.name,
      });
      changed();
      return { type: "RemoteUserSaved", user: structuredClone(found) };
    }
    case "DisableTotp": {
      const found = user(request.user_id);
      if (found === undefined) return notFoundUser(request.user_id);
      forget(s.secrets.totp, found.id);
      found.totp_enabled = false;
      writeAudit(core, {
        kind: "totp-changed",
        outcome: "success",
        user_id: found.id,
        user_name: found.name,
      });
      changed();
      return { type: "RemoteUserSaved", user: structuredClone(found) };
    }
    case "UnlockRemoteUser": {
      const found = user(request.user_id);
      if (found === undefined) return notFoundUser(request.user_id);
      found.locked_until = null;
      forget(s.failures, `user:${found.name.toLowerCase()}`);
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
        for (const [credential, id] of Object.entries(s.secrets.credentials)) {
          if (id === request.passkey_id) forget(s.secrets.credentials, credential);
        }
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
      for (const session of s.sessions.filter((x) => x.device_id === device.id)) {
        session.device_name = device.name;
        session.grant = intersectGrants(session.grant, device.grant);
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
    case "CreatePairingCode": {
      const found = user(request.user_id);
      if (found === undefined) return notFoundUser(request.user_id);
      if (found.disabled) return errorResponse("invalid_request", "the user is disabled");
      const ttl = Math.min(Math.max(request.ttl_minutes ?? 10, 1), 60);
      const code = randomFrom(CODE_ALPHABET, 8);
      const pairing: PairingCode = {
        id: core.nextId("pair-"),
        user_id: found.id,
        device_name: request.device_name?.trim() || null,
        grant:
          request.grant === null
            ? structuredClone(found.grant)
            : intersectGrants(request.grant, found.grant),
        created_at: now(),
        expires_at: later(ttl * 60),
        state: "pending",
        device_id: null,
      };
      s.pairing.push(pairing);
      s.secrets.pairingCodes[pairing.id] = code;
      writeAudit(core, {
        kind: "pairing-created",
        outcome: "success",
        user_id: found.id,
        user_name: found.name,
        name: pairing.device_name,
      });
      changed();
      const status = mockStatus(s);
      return {
        type: "PairingCodeCreated",
        pairing: structuredClone(pairing),
        code: `${code.slice(0, 4)}-${code.slice(4)}`,
        url: status.urls[0] ?? null,
        certificate_fingerprint: status.certificate?.fingerprint_sha256 ?? null,
      };
    }
    case "ListPairingCodes":
      expirePairingCodes(s);
      return { type: "PairingCodes", codes: structuredClone(s.pairing) };
    case "CancelPairingCode": {
      const code = s.pairing.find((p) => p.id === request.pairing_id);
      if (code === undefined)
        return errorResponse("not_found", `pairing code ${request.pairing_id}`);
      if (code.state === "pending") {
        code.state = "cancelled";
        forget(s.secrets.pairingCodes, code.id);
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
  }
}
