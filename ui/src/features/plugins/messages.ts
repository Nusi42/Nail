/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/plugins.ftl` (or, for texts
 * the core builds, `locales/en/core-plugins.ftl`); ids built at runtime are
 * listed by `runtimeIds`.
 */
import type {
  PluginLogLevel,
  PluginLogSource,
  PluginManifestIssueKind,
  PluginPermission,
  PluginRunState,
  PluginStopReason,
} from "../../api/generated";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["plugin-"];

/** Literals with those prefixes that are not message ids (part, widget, dialog names). */
export const notMessages: readonly string[] = [
  "plugin-panel",
  "plugin-manager",
  "plugin-install",
  "plugin-dialog",
  "plugin-permissions",
  "plugin-placements",
  "plugin-logs",
  "plugin-settings",
];

/** The permissions without an argument (`PluginPermission::FIXED`). */
export const FIXED_PERMISSIONS: readonly PluginPermission[] = [
  "mail.read",
  "mail.read.body",
  "calendar.read",
  "ui.dialogs",
  "notifications",
  "storage",
];

/** The Fluent id of a permission's label (`PluginPermission::label`). */
export function permissionMessageId(permission: PluginPermission): string {
  return permission.startsWith("network:")
    ? "plugin-permission-network"
    : `plugin-permission-${permission.replaceAll(".", "-")}`;
}

/** The arguments of a permission's label (`$domain` of `network:<host>`). */
export function permissionArgs(permission: PluginPermission): Record<string, string> {
  return permission.startsWith("network:") ? { domain: permission.slice("network:".length) } : {};
}

const STATES: readonly PluginRunState[] = [
  "disabled",
  "starting",
  "running",
  "stopped",
  "crashed",
  "incompatible",
];
const LEVELS: readonly PluginLogLevel[] = ["debug", "info", "warn", "error"];
const SOURCES: readonly PluginLogSource[] = ["plugin", "host", "api"];
const STOPS: readonly PluginStopReason[] = [
  "disabled",
  "uninstalled",
  "reloaded",
  "shutdown",
  "error",
  "cpu-budget",
  "memory-limit",
];
const ISSUES: readonly PluginManifestIssueKind[] = [
  "too-large",
  "invalid-json",
  "invalid-id",
  "invalid-name",
  "invalid-version",
  "unsupported-api-version",
  "invalid-main",
  "invalid-homepage",
  "unknown-permission",
  "permission-not-in-preview",
  "invalid-network-domain",
  "duplicate-permission",
  "body-without-read",
  "invalid-command",
  "duplicate-command",
  "invalid-placement",
  "unknown-command",
  "unknown-panel",
  "invalid-key",
  "invalid-panel",
  "duplicate-panel",
  "invalid-setting",
  "duplicate-setting",
  "invalid-locale",
  "too-many-contributions",
];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  const network: PluginPermission = "network:example.com";
  return [
    ...[...FIXED_PERMISSIONS, network].map(permissionMessageId),
    "window-plugin-install",
    "window-plugin-manager",
    ...STATES.map((s) => `plugin-state-${s}`),
    ...LEVELS.map((l) => `plugin-log-level-${l}`),
    ...SOURCES.map((s) => `plugin-log-source-${s}`),
    ...STOPS.map((s) => `plugin-stopped-${s}`),
    ...ISSUES.map((k) => `plugin-manifest-${k}`),
    ...["required", "email", "url", "number", "too-long", "date-range"].map(
      (i) => `plugin-field-${i}`,
    ),
    ...["file", "url", "dev"].flatMap((k) => [
      `plugin-install-source-${k}`,
      `plugin-install-location-${k}`,
    ]),
    ...["permissions", "settings", "log", "layout"].map((s) => `plugin-manager-show-${s}`),
    // Layout labels for the integrator's default layout (`@plugin.…`).
    "plugin-manager-tab",
    "plugin-action-install",
    "plugin-action-load-dev",
    "plugin-action-manage",
  ];
}
