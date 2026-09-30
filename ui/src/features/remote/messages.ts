/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/remote.ftl`; ids built at
 * runtime are listed by `runtimeIds`.
 */
import {
  AUDIT_KINDS,
  AUDIT_OUTCOMES,
  AUTH_FAILURES,
  AUTH_METHODS,
  BIND_CHOICES,
  CONTENT_SCOPES,
  END_REASONS,
  ISSUES,
  PAIRING_STATES,
  PRESETS,
  TLS_CHOICES,
  UI_STATES,
} from "./model";
import { PAIR_DIALOG } from "./PairDevice";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["remote-"];

/** Literals with those prefixes that are not message ids (part names, widget kinds). */
export const notMessages: readonly string[] = [
  // Widget kinds.
  "remote-settings",
  "remote-users",
  "remote-devices",
  "remote-audit",
  "remote-content",
  "remote-content-allows",
  // Dialog name.
  "remote-pair",
  // Part names without a dot.
  "remote-login",
  "remote-reauth",
  "remote-confirm",
  "remote-fingerprint",
  "remote-qr",
  "remote-totp",
  "remote-passkeys",
  "remote-pairing",
  "remote-sessions",
  "remote-allows",
  "remote-alerts",
  // `RemoteAuditKind` value that happens to match the shared `settings-` prefix.
  "settings-changed",
];

/** Settings tabs and actions the default layout labels (`@remote.…`). */
const LAYOUT_LABELS = [
  "remote-settings-tab",
  "remote-settings-tab-users",
  "remote-settings-tab-devices",
  "remote-settings-tab-audit",
  "remote-settings-tab-privacy",
  "remote-action-pair",
  "remote-action-load-content",
  "remote-action-logout",
];

const PASSKEY_PROBLEMS = ["cancelled", "not-supported", "failed"];
const CERTIFICATE_SOURCES = ["self-signed", "acme"];
const LOCK_REASONS = ["pentest-pending"];
const ALERT_KINDS = ["new-device", "lockout"];

/** Message ids built at runtime (e.g. `remote-audit-kind-<kind>`). */
export function runtimeIds(): string[] {
  return [
    ...AUTH_FAILURES.map((r) => `remote-login-failed-${r}`),
    ...PASSKEY_PROBLEMS.map((p) => `remote-passkey-${p}`),
    ...UI_STATES.map((s) => `remote-state-${s}`),
    ...CERTIFICATE_SOURCES.map((s) => `remote-certificate-${s}`),
    ...LOCK_REASONS.map((r) => `remote-internet-lock-${r}`),
    ...BIND_CHOICES.map((c) => `remote-bind-${c}`),
    ...TLS_CHOICES.flatMap((m) => [`remote-tls-${m}`, `remote-tls-${m}-hint`]),
    ...ISSUES.map((i) => `remote-issue-${i}`),
    ...[...PRESETS, "custom"].flatMap((p) => [`remote-preset-${p}`, `remote-preset-${p}-hint`]),
    ...PAIRING_STATES.map((s) => `remote-pairing-state-${s}`),
    ...AUTH_METHODS.map((m) => `remote-method-${m}`),
    ...END_REASONS.map((r) => `remote-end-${r}`),
    ...AUDIT_KINDS.map((k) => `remote-audit-kind-${k}`),
    ...AUDIT_OUTCOMES.map((o) => `remote-audit-outcome-${o}`),
    ...ALERT_KINDS.map((k) => `remote-alert-title-${k}`),
    ...CONTENT_SCOPES.flatMap((s) => [
      `remote-content-allowed-${s}`,
      `remote-content-block-${s}`,
      `remote-allows-scope-${s}`,
    ]),
    "remote-content-allowed-once",
    "remote-content-block-once",
    `window-${PAIR_DIALOG}`,
    ...LAYOUT_LABELS,
  ];
}
