/**
 * Pure helpers of the remote-UI and privacy UI (ADR 0036): access presets
 * (`RemoteAccessPreset::grant`), the settings checks the core runs
 * (`RemoteUiSettings::issues`, mirrored so the form can explain a problem
 * before saving), fingerprint and pairing-code formatting, and the value
 * lists the Fluent ids are built from.
 */
import type {
  AuthFailure,
  McpScope,
  RemoteAccessPreset,
  RemoteAuditKind,
  RemoteAuditOutcome,
  RemoteAuthMethod,
  RemoteContentScope,
  RemoteGrant,
  RemoteTlsMode,
  RemoteUiSettings,
  RemoteUiSettingsIssue,
  RemoteUiState,
  SessionEndReason,
} from "../../api/generated";

/** `McpScope::ALL`: the order scopes are kept in. */
export const SCOPE_ORDER: readonly McpScope[] = [
  "read",
  "raw-source",
  "attachments",
  "encrypted",
  "context",
  "organize",
  "permanent-delete",
  "draft",
  "send",
  "calendar",
  "pim",
  "rules",
];

/** The presets, narrowest first (`RemoteAccessPreset::ALL`). */
export const PRESETS: readonly RemoteAccessPreset[] = ["read-only", "no-send", "full"];

const READ: readonly McpScope[] = ["read", "raw-source", "attachments"];
const WRITE: readonly McpScope[] = [
  "organize",
  "permanent-delete",
  "draft",
  "calendar",
  "pim",
  "rules",
];
/** Scopes a preset never contains; they are explicit extra choices. */
export const EXTRA_SCOPES: readonly McpScope[] = ["encrypted", "context"];

function sortScopes(scopes: readonly McpScope[]): McpScope[] {
  return [...new Set(scopes)].sort((a, b) => SCOPE_ORDER.indexOf(a) - SCOPE_ORDER.indexOf(b));
}

/** The grant of a preset (all accounts), plus the chosen extras. */
export function presetGrant(
  preset: RemoteAccessPreset,
  extras: { admin?: boolean; scopes?: readonly McpScope[] } = {},
): RemoteGrant {
  const scopes: McpScope[] = [...READ];
  if (preset !== "read-only") scopes.push(...WRITE);
  if (preset === "full") scopes.push("send");
  scopes.push(...(extras.scopes ?? []).filter((s) => EXTRA_SCOPES.includes(s)));
  return {
    mail: {
      scopes: sortScopes(scopes),
      all_accounts: true,
      accounts: [],
      excluded_folders: [],
      skip_draft_confirmation: false,
    },
    admin: extras.admin ?? false,
  };
}

/**
 * The preset a grant corresponds to (ignoring `admin` and the extra
 * scopes), or `custom` when it matches none (e.g. narrowed to accounts).
 */
export function presetOf(grant: RemoteGrant): RemoteAccessPreset | "custom" {
  const base = grant.mail.scopes.filter((s) => !EXTRA_SCOPES.includes(s));
  const narrowed = !grant.mail.all_accounts || grant.mail.excluded_folders.length > 0;
  if (narrowed) return "custom";
  for (const preset of PRESETS) {
    const scopes = presetGrant(preset).mail.scopes;
    if (scopes.length === base.length && scopes.every((s) => base.includes(s))) return preset;
  }
  return "custom";
}

/** The extra scopes a grant holds. */
export function extrasOf(grant: RemoteGrant): McpScope[] {
  return grant.mail.scopes.filter((s) => EXTRA_SCOPES.includes(s));
}

/** Whether `inner` allows nothing `outer` does not (`RemoteGrant::is_within`). */
export function grantIsWithin(inner: RemoteGrant, outer: RemoteGrant): boolean {
  if (inner.admin && !outer.admin) return false;
  if (!inner.mail.scopes.every((s) => outer.mail.scopes.includes(s))) return false;
  if (inner.mail.all_accounts && !outer.mail.all_accounts) return false;
  if (
    !outer.mail.all_accounts &&
    !inner.mail.accounts.every((a) => outer.mail.accounts.includes(a))
  )
    return false;
  return outer.mail.excluded_folders.every((f) => inner.mail.excluded_folders.includes(f));
}

// ---------------------------------------------------------------------------
// Settings

/** Bind-address choices of the settings form. */
export const BIND_CHOICES = ["loopback", "lan", "custom"] as const;
export type BindChoice = (typeof BIND_CHOICES)[number];

export const LOOPBACK_ADDRESS = "127.0.0.1";
export const ALL_INTERFACES = "0.0.0.0";

/** TLS modes offered outside internet mode (ACME needs internet mode). */
export const TLS_CHOICES: readonly RemoteTlsMode[] = ["self-signed", "off"];

export const ISSUES: readonly RemoteUiSettingsIssue[] = [
  "invalid-bind-address",
  "invalid-port",
  "tls-required",
  "internet-mode-locked",
  "acme-needs-internet-mode",
  "acme-incomplete",
  "invalid-network",
  "invalid-host-name",
  "invalid-session-limits",
];

export const UI_STATES: readonly RemoteUiState[] = ["stopped", "starting", "running", "failed"];

export function bindChoiceOf(address: string): BindChoice {
  if (address === LOOPBACK_ADDRESS) return "loopback";
  if (address === ALL_INTERFACES) return "lan";
  return "custom";
}

function parseIpv4(text: string): number[] | null {
  const parts = text.split(".");
  if (parts.length !== 4) return null;
  const bytes = parts.map((p) => (/^\d{1,3}$/.test(p) ? Number(p) : Number.NaN));
  return bytes.every((b) => b >= 0 && b <= 255) ? bytes : null;
}

function isIpv6(text: string): boolean {
  if (!/^[0-9a-fA-F:.]+$/.test(text) || !text.includes(":")) return false;
  const doubles = text.split("::").length - 1;
  if (doubles > 1) return false;
  let groups = text.split(":").filter((g) => g !== "");
  const last = groups.at(-1);
  let extra = 0;
  if (last !== undefined && last.includes(".")) {
    if (parseIpv4(last) === null) return false;
    groups = groups.slice(0, -1);
    extra = 2;
  }
  if (!groups.every((g) => /^[0-9a-fA-F]{1,4}$/.test(g))) return false;
  const count = groups.length + extra;
  return doubles === 1 ? count < 8 : count === 8;
}

/** Whether `text` is an IPv4 or IPv6 address. */
export function isIpAddress(text: string): boolean {
  const t = text.trim();
  return parseIpv4(t) !== null || isIpv6(t);
}

/** Whether `text` is a loopback address (`127.0.0.0/8`, `::1`). */
export function isLoopbackAddress(text: string): boolean {
  const t = text.trim();
  const v4 = parseIpv4(t);
  if (v4 !== null) return v4[0] === 127;
  return t === "::1" || /^(0{1,4}:){7}0{0,3}1$/.test(t) || /^(0{1,4}:){1,6}:0{0,3}1$/.test(t);
}

function isCidr(text: string): boolean {
  const [ip, prefix, ...rest] = text.trim().split("/");
  if (ip === undefined || prefix === undefined || rest.length > 0) return false;
  if (!/^\d{1,3}$/.test(prefix) || !isIpAddress(ip)) return false;
  return Number(prefix) <= (parseIpv4(ip) !== null ? 32 : 128);
}

function isDnsName(name: string): boolean {
  const n = name.endsWith(".") ? name.slice(0, -1) : name;
  if (n === "" || n.length > 253 || isIpAddress(n)) return false;
  const labels = n.split(".");
  const ok = labels.every(
    (l) =>
      l.length > 0 &&
      l.length <= 63 &&
      !l.startsWith("-") &&
      !l.endsWith("-") &&
      /^[A-Za-z0-9-]+$/.test(l),
  );
  return ok && !/^\d+$/.test(labels.at(-1) ?? "");
}

/**
 * Everything wrong with `settings`, like the core's
 * `RemoteUiSettings::issues` with internet mode locked (always in M2).
 */
export function settingsIssues(settings: RemoteUiSettings): RemoteUiSettingsIssue[] {
  const issues: RemoteUiSettingsIssue[] = [];
  if (!isIpAddress(settings.bind_address)) issues.push("invalid-bind-address");
  else if (!isLoopbackAddress(settings.bind_address) && settings.tls === "off") {
    issues.push("tls-required");
  }
  if (!Number.isInteger(settings.port) || settings.port <= 0 || settings.port > 65535) {
    issues.push("invalid-port");
  }
  if (settings.internet_mode) issues.push("internet-mode-locked");
  if (settings.tls === "acme") {
    if (!settings.internet_mode) issues.push("acme-needs-internet-mode");
    if (settings.acme.domains.length === 0 || !settings.acme.accept_terms) {
      issues.push("acme-incomplete");
    }
  }
  if (!settings.allowed_networks.every(isCidr)) issues.push("invalid-network");
  if (settings.host_name !== null && !isDnsName(settings.host_name)) {
    issues.push("invalid-host-name");
  }
  const { session_idle_minutes: idle, session_max_hours: max, reauth_minutes: reauth } = settings;
  if (idle <= 0 || max <= 0 || reauth <= 0 || idle > max * 60) {
    issues.push("invalid-session-limits");
  }
  return issues;
}

/** The issue a core error message names (`invalid_request` / `forbidden`). */
export function issueOfMessage(message: string): RemoteUiSettingsIssue | null {
  return ISSUES.find((issue) => message.includes(issue)) ?? null;
}

// ---------------------------------------------------------------------------
// Formatting

/**
 * A certificate fingerprint for people to compare: uppercase hex in
 * groups of four (`AB12 CD34 …`). Other input is returned trimmed.
 */
export function groupFingerprint(hex: string): string {
  const clean = hex.replace(/[\s:]/g, "").toUpperCase();
  if (!/^[0-9A-F]+$/.test(clean)) return hex.trim();
  return clean.match(/.{1,4}/g)?.join(" ") ?? clean;
}

/** A pairing code as typed, without separators and in upper case. */
export function normalizePairingCode(input: string): string {
  return input.replace(/[\s\-_.]/g, "").toUpperCase();
}

/** A pairing code for display, in groups of four (`K7QM-2WXR`). */
export function formatPairingCode(code: string): string {
  const clean = normalizePairingCode(code);
  return clean.match(/.{1,4}/g)?.join("-") ?? clean;
}

/** A date-time (the raw string when it does not parse). */
export function formatTime(iso: string | null, locale: string): string {
  if (iso === null) return "";
  const date = new Date(iso);
  return Number.isNaN(date.getTime())
    ? iso
    : new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeStyle: "short" }).format(date);
}

/** Whole seconds from `now` until `iso` (0 when past or invalid). */
export function secondsUntil(iso: string, now: number = Date.now()): number {
  const at = new Date(iso).getTime();
  return Number.isNaN(at) ? 0 : Math.max(0, Math.ceil((at - now) / 1000));
}

// ---------------------------------------------------------------------------
// Values the Fluent ids are built from

export const AUTH_FAILURES: readonly AuthFailure[] = [
  "invalid-credentials",
  "invalid-code",
  "challenge-expired",
  "rate-limited",
  "locked-out",
  "not-paired",
  "not-logged-in",
  "reauth-required",
  "disabled",
  "bad-request",
];
export const AUTH_METHODS: readonly RemoteAuthMethod[] = [
  "passkey",
  "password",
  "password-totp",
  "pairing",
];
export const END_REASONS: readonly SessionEndReason[] = [
  "logout",
  "revoked",
  "expired",
  "device-revoked",
  "user-changed",
];
export const AUDIT_KINDS: readonly RemoteAuditKind[] = [
  "login",
  "reauth",
  "logout",
  "pairing-created",
  "pairing",
  "lockout",
  "password-changed",
  "totp-changed",
  "passkey-changed",
  "session-ended",
  "device-changed",
  "user-changed",
  "settings-changed",
  "request",
];
export const AUDIT_OUTCOMES: readonly RemoteAuditOutcome[] = ["success", "failure", "denied"];
export const CONTENT_SCOPES: readonly RemoteContentScope[] = ["message", "sender", "domain"];
export const PAIRING_STATES = ["pending", "used", "expired", "cancelled"] as const;
/** Lifetimes offered for a pairing code, in minutes. */
export const PAIRING_TTLS: readonly number[] = [5, 10, 30];
/** Shortest password the forms accept (the core may ask for more). */
export const MIN_PASSWORD_LENGTH = 12;
