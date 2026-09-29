/**
 * A mock of the login protocol (`POST /api/auth`, ADR 0036) on the same
 * state as the mock core (`data.features.remote`), so a pairing code made
 * in the desktop UI can be entered on the login page in tests and in the
 * `?mock=1` dev server. Each instance is one browser (its cookies).
 *
 * Behaves like WP41 is specified: pairing codes are single-use and
 * expire; failures are coarse; failed attempts are counted per address
 * and per login name, and too many lock the name / address out for a
 * while (with a `lockout` alert); a TOTP step is needed when enrolled; a
 * passkey challenge is single-use; logins and pairings are audited and a
 * new device raises a `new-device` alert. Passkey "signatures" are not
 * checked: a credential whose `id` is a registered credential id passes.
 */
import type {
  AuthFailure,
  AuthPurpose,
  AuthRequest,
  AuthResponse,
  AuthStatus,
  RemoteAuthMethod,
  RemoteDevice,
  RemoteSession,
  RemoteUser,
} from "../../api/generated";
import type { MockCore } from "../../api/mockMail";
import type { AuthTransport } from "./authClient";
import {
  MOCK_LOCKOUT_SECS,
  MOCK_MAX_FAILURES,
  MOCK_TOTP_CODE,
  findPairingCode,
  forget,
  intersectGrants,
  later,
  mockStatus,
  now,
  remoteState,
  writeAudit,
  type RemoteMockState,
} from "./mock";

interface Challenge {
  kind: "login" | "register";
  purpose: AuthPurpose;
  userId: string | null;
  expires: number;
  /** Name of the passkey being registered. */
  name?: string;
}

export interface MockAuthOptions {
  ip?: string;
  userAgent?: string;
  /** Artificial latency in ms. */
  latencyMs?: number;
}

function b64url(bytes: Uint8Array): string {
  let text = "";
  for (const b of bytes) text += String.fromCharCode(b);
  return btoa(text).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function randomB64(length = 32): string {
  const bytes = new Uint8Array(length);
  crypto.getRandomValues(bytes);
  return b64url(bytes);
}

function credentialId(credential: unknown): string | null {
  if (credential !== null && typeof credential === "object" && "id" in credential) {
    const id = (credential as { id: unknown }).id;
    return typeof id === "string" ? id : null;
  }
  return null;
}

/** One browser talking to the mock login route. */
export class MockAuthServer implements AuthTransport {
  /** The device cookie (device id), `null` = not paired. */
  deviceId: string | null = null;
  /** The session cookie (session id), `null` = logged out. */
  sessionId: string | null = null;
  /** Every request received, in order. */
  readonly requests: AuthRequest[] = [];
  private readonly core: MockCore;
  private readonly ip: string;
  private readonly userAgent: string;
  private readonly latencyMs: number;
  private readonly challenges = new Map<string, Challenge>();
  private csrf = randomB64(16);

  constructor(core: MockCore, options: MockAuthOptions = {}) {
    this.core = core;
    this.ip = options.ip ?? "192.168.1.42";
    this.userAgent = options.userAgent ?? "Mozilla/5.0 (Mock) Firefox/140.0";
    this.latencyMs = options.latencyMs ?? 0;
  }

  private get s(): RemoteMockState {
    return remoteState(this.core);
  }

  async send(request: AuthRequest): Promise<AuthResponse> {
    this.requests.push(request);
    if (this.latencyMs > 0) await new Promise((r) => setTimeout(r, this.latencyMs));
    else await Promise.resolve();
    return this.answer(request);
  }

  private device(): RemoteDevice | null {
    const d = this.s.devices.find((x) => x.id === this.deviceId);
    return d !== undefined && d.revoked_at === null ? d : null;
  }

  /** The valid session of this browser, if any. */
  session(): RemoteSession | null {
    const found = this.s.sessions.find((x) => x.id === this.sessionId);
    if (found === undefined || found.ended_at !== null) return null;
    if (new Date(found.expires_at).getTime() <= Date.now()) return null;
    return this.device() === null ? null : found;
  }

  private status(): AuthStatus {
    const session = this.session();
    const reauth = session?.reauth_at ?? session?.created_at ?? null;
    const reauthUntil =
      reauth === null
        ? null
        : new Date(new Date(reauth).getTime() + this.s.settings.reauth_minutes * 60_000);
    return {
      authenticated: session !== null,
      paired: this.device() !== null,
      session: structuredClone(session),
      reauth_until:
        reauthUntil !== null && reauthUntil.getTime() > Date.now()
          ? reauthUntil.toISOString().replace(/\.\d{3}Z$/, "Z")
          : null,
      csrf_token: this.csrf,
      methods: { passkey: true, password: true },
      certificate_fingerprint: mockStatus(this.s).certificate?.fingerprint_sha256 ?? null,
    };
  }

  private failed(reason: AuthFailure, retry: number | null = null): AuthResponse {
    return { type: "Failed", reason, retry_after_secs: retry };
  }

  /** Seconds the address or name is still locked out, 0 = not. */
  private lockedFor(user: RemoteUser | undefined): number {
    const until = [this.s.lockedIps[this.ip], user?.locked_until ?? undefined]
      .filter((x): x is string => x !== undefined && x !== null)
      .map((x) => new Date(x).getTime() - Date.now())
      .filter((ms) => ms > 0);
    return until.length === 0 ? 0 : Math.ceil(Math.max(...until) / 1000);
  }

  /** Counts a failure; returns the lockout in seconds when it starts one. */
  private countFailure(user: RemoteUser | undefined, name: string | null): number | null {
    const keys = [`ip:${this.ip}`, ...(name === null ? [] : [`user:${name.toLowerCase()}`])];
    let locked: number | null = null;
    for (const key of keys) {
      const count = (this.s.failures[key] ?? 0) + 1;
      this.s.failures[key] = count;
      if (count >= MOCK_MAX_FAILURES) {
        const until = later(MOCK_LOCKOUT_SECS);
        if (key.startsWith("ip:")) this.s.lockedIps[this.ip] = until;
        else if (user !== undefined) user.locked_until = until;
        this.s.failures[key] = 0;
        locked = MOCK_LOCKOUT_SECS;
        writeAudit(this.core, {
          kind: "lockout",
          outcome: "denied",
          user_id: user?.id ?? null,
          user_name: name,
          ip: this.ip,
          user_agent: this.userAgent,
        });
        this.core.emit({
          type: "RemoteSecurityAlert",
          alert: {
            kind: "lockout",
            user_name: key.startsWith("user:") ? name : null,
            ip: key.startsWith("ip:") ? this.ip : null,
            until,
          },
        });
      }
    }
    return locked;
  }

  private clearFailures(name: string): void {
    forget(this.s.failures, `ip:${this.ip}`);
    forget(this.s.failures, `user:${name.toLowerCase()}`);
  }

  private startSession(user: RemoteUser, device: RemoteDevice, method: RemoteAuthMethod) {
    const at = now();
    const session: RemoteSession = {
      id: this.core.nextId("rsess-"),
      user_id: user.id,
      user_name: user.name,
      device_id: device.id,
      device_name: device.name,
      auth_method: method,
      created_at: at,
      last_seen_at: at,
      expires_at: later(this.s.settings.session_max_hours * 3600),
      ip: this.ip,
      user_agent: this.userAgent,
      reauth_at: null,
      grant: intersectGrants(device.grant, user.grant),
      ended_at: null,
      end_reason: null,
    };
    this.s.sessions.push(session);
    this.sessionId = session.id;
    user.last_login_at = at;
    device.last_seen_at = at;
    device.last_ip = this.ip;
    device.user_agent = this.userAgent;
    this.core.emit({ type: "RemoteAccessChanged" });
    return session;
  }

  private audit(
    kind: "login" | "reauth" | "pairing" | "logout" | "passkey-changed",
    ok: boolean,
    user: RemoteUser | undefined,
    extra: { method?: RemoteAuthMethod; name?: string | null; error?: "auth_failed" } = {},
  ) {
    writeAudit(this.core, {
      kind,
      outcome: ok ? "success" : "failure",
      user_id: user?.id ?? null,
      user_name: user?.name ?? extra.name ?? null,
      device_id: this.deviceId,
      session_id: this.session()?.id ?? null,
      ip: this.ip,
      user_agent: this.userAgent,
      method: extra.method ?? null,
      name: kind === "passkey-changed" ? (extra.name ?? null) : null,
      error: ok ? null : (extra.error ?? "auth_failed"),
    });
  }

  private loggedIn(
    purpose: AuthPurpose,
    user: RemoteUser,
    device: RemoteDevice,
    method: RemoteAuthMethod,
  ): AuthResponse {
    this.clearFailures(user.name);
    if (purpose === "reauth") {
      const session = this.session();
      if (session === null || session.user_id !== user.id) return this.failed("not-logged-in");
      const stored = this.s.sessions.find((x) => x.id === session.id);
      if (stored !== undefined) stored.reauth_at = now();
      this.audit("reauth", true, user, { method });
      return { type: "Reauthenticated", reauth_until: later(this.s.settings.reauth_minutes * 60) };
    }
    const session = this.startSession(user, device, method);
    this.audit("login", true, user, { method });
    return { type: "LoggedIn", session: structuredClone(session) };
  }

  private answer(request: AuthRequest): AuthResponse {
    const s = this.s;
    switch (request.type) {
      case "Status":
        return { type: "Status", status: this.status() };
      case "Pair": {
        const locked = this.lockedFor(undefined);
        if (locked > 0) return this.failed("rate-limited", locked);
        const pairing = findPairingCode(s, request.code);
        const user = s.users.find((u) => u.id === pairing?.user_id);
        if (pairing === undefined || user === undefined || user.disabled) {
          const lock = this.countFailure(undefined, null);
          this.audit("pairing", false, undefined);
          return lock === null ? this.failed("invalid-code") : this.failed("rate-limited", lock);
        }
        const device: RemoteDevice = {
          id: this.core.nextId("rdev-"),
          user_id: user.id,
          name: pairing.device_name ?? (request.device_name.trim() || "Browser"),
          paired_at: now(),
          last_seen_at: null,
          user_agent: this.userAgent,
          last_ip: this.ip,
          grant: intersectGrants(pairing.grant, user.grant),
          revoked_at: null,
        };
        s.devices.push(device);
        pairing.state = "used";
        pairing.device_id = device.id;
        forget(s.secrets.pairingCodes, pairing.id);
        this.deviceId = device.id;
        this.audit("pairing", true, user, { method: "pairing" });
        this.core.emit({
          type: "RemoteSecurityAlert",
          alert: {
            kind: "new-device",
            device_id: device.id,
            device_name: device.name,
            user_name: user.name,
            ip: this.ip,
          },
        });
        return this.loggedIn("login", user, device, "pairing");
      }
      case "Password": {
        const device = this.device();
        if (device === null) return this.failed("not-paired");
        const user = s.users.find(
          (u) => u.name.toLowerCase() === request.user_name.trim().toLowerCase(),
        );
        const locked = this.lockedFor(user);
        if (locked > 0) {
          return this.failed(user?.locked_until ? "locked-out" : "rate-limited", locked);
        }
        const password = user === undefined ? undefined : s.secrets.passwords[user.id];
        const valid =
          user !== undefined &&
          !user.disabled &&
          password !== undefined &&
          password === request.password &&
          device.user_id === user.id;
        if (!valid) {
          const lock = this.countFailure(user, request.user_name.trim());
          this.audit(request.purpose === "reauth" ? "reauth" : "login", false, user, {
            method: "password",
            name: request.user_name.trim(),
          });
          if (lock !== null) return this.failed("locked-out", lock);
          return this.failed(user?.disabled ? "disabled" : "invalid-credentials");
        }
        if (user.totp_enabled) {
          if (request.totp_code === null) return { type: "TotpRequired" };
          if (request.totp_code.replace(/\s/g, "") !== MOCK_TOTP_CODE) {
            const lock = this.countFailure(user, user.name);
            this.audit("login", false, user, { method: "password-totp" });
            return lock === null ? this.failed("invalid-code") : this.failed("locked-out", lock);
          }
          return this.loggedIn(request.purpose, user, device, "password-totp");
        }
        return this.loggedIn(request.purpose, user, device, "password");
      }
      case "PasskeyStart": {
        const device = this.device();
        if (device === null) return this.failed("not-paired");
        const locked = this.lockedFor(undefined);
        if (locked > 0) return this.failed("rate-limited", locked);
        const user = s.users.find((u) => u.id === device.user_id);
        const id = randomB64(12);
        this.challenges.set(id, {
          kind: "login",
          purpose: request.purpose,
          userId: user?.id ?? null,
          expires: Date.now() + 120_000,
        });
        const allow = Object.entries(s.secrets.credentials)
          .filter(([, passkey]) => user?.passkeys.some((p) => p.id === passkey))
          .map(([credential]) => ({ id: credential, type: "public-key" }));
        return {
          type: "PasskeyChallenge",
          challenge_id: id,
          options: {
            challenge: randomB64(),
            rpId: s.settings.host_name ?? "nail-desktop.local",
            timeout: 120_000,
            userVerification: "required",
            allowCredentials: request.user_name === null ? [] : allow,
          },
        };
      }
      case "PasskeyFinish": {
        const challenge = this.challenges.get(request.challenge_id);
        this.challenges.delete(request.challenge_id);
        const device = this.device();
        if (device === null) return this.failed("not-paired");
        if (
          challenge === undefined ||
          challenge.kind !== "login" ||
          challenge.expires < Date.now()
        ) {
          return this.failed("challenge-expired");
        }
        const passkeyId = s.secrets.credentials[credentialId(request.credential) ?? ""];
        const user = s.users.find((u) => u.passkeys.some((p) => p.id === passkeyId));
        if (user === undefined || user.disabled || device.user_id !== user.id) {
          const lock = this.countFailure(undefined, null);
          this.audit("login", false, undefined, { method: "passkey" });
          return lock === null
            ? this.failed("invalid-credentials")
            : this.failed("rate-limited", lock);
        }
        const key = user.passkeys.find((p) => p.id === passkeyId);
        if (key !== undefined) key.last_used_at = now();
        return this.loggedIn(challenge.purpose, user, device, "passkey");
      }
      case "PasskeyRegisterStart": {
        const session = this.session();
        if (session === null) return this.failed("not-logged-in");
        const user = s.users.find((u) => u.id === session.user_id);
        if (user === undefined) return this.failed("not-logged-in");
        if (request.name.trim() === "") return this.failed("bad-request");
        const id = randomB64(12);
        this.challenges.set(id, {
          kind: "register",
          purpose: "login",
          userId: user.id,
          expires: Date.now() + 120_000,
          name: request.name.trim(),
        });
        return {
          type: "PasskeyChallenge",
          challenge_id: id,
          options: {
            challenge: randomB64(),
            rp: { id: s.settings.host_name ?? "nail-desktop.local", name: "Nail" },
            user: {
              id: b64url(new TextEncoder().encode(user.id)),
              name: user.name,
              displayName: user.display_name ?? user.name,
            },
            pubKeyCredParams: [
              { type: "public-key", alg: -7 },
              { type: "public-key", alg: -257 },
            ],
            timeout: 120_000,
            authenticatorSelection: { residentKey: "preferred", userVerification: "required" },
            excludeCredentials: [],
            attestation: "none",
          },
        };
      }
      case "PasskeyRegisterFinish": {
        const challenge = this.challenges.get(request.challenge_id);
        this.challenges.delete(request.challenge_id);
        const session = this.session();
        if (session === null) return this.failed("not-logged-in");
        if (
          challenge === undefined ||
          challenge.kind !== "register" ||
          challenge.userId !== session.user_id ||
          challenge.expires < Date.now()
        ) {
          return this.failed("challenge-expired");
        }
        const credential = credentialId(request.credential);
        const user = s.users.find((u) => u.id === session.user_id);
        if (credential === null || user === undefined) return this.failed("bad-request");
        const passkey = {
          id: this.core.nextId("passkey-"),
          name: challenge.name ?? "",
          created_at: now(),
          last_used_at: null,
          device_id: session.device_id,
        };
        user.passkeys.push(passkey);
        s.secrets.credentials[credential] = passkey.id;
        this.audit("passkey-changed", true, user, { name: passkey.name });
        this.core.emit({ type: "RemoteAccessChanged" });
        return { type: "PasskeyRegistered", passkey: structuredClone(passkey) };
      }
      case "Logout": {
        const session = this.session();
        if (session !== null) {
          const stored = s.sessions.find((x) => x.id === session.id);
          if (stored !== undefined) {
            stored.ended_at = now();
            stored.end_reason = "logout";
          }
          const user = s.users.find((u) => u.id === session.user_id);
          this.audit("logout", true, user);
          this.core.emit({ type: "RemoteSessionEnded", session_id: session.id, reason: "logout" });
          this.core.emit({ type: "RemoteAccessChanged" });
        }
        this.sessionId = null;
        return { type: "LoggedOut" };
      }
    }
  }
}
