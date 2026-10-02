/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/plugins.ftl` (or, for texts
 * the core builds, `locales/en/core-plugins.ftl`); ids built at runtime are
 * listed by `runtimeIds`.
 */
import type {
  ComposerEditOp,
  PluginHookKind,
  PluginJobOutcome,
  PluginLogLevel,
  PluginLogSource,
  PluginManifestIssueKind,
  PluginPermission,
  PluginRunState,
  PluginStopReason,
} from "../../api/generated";
import { contributionMessageId, hookMessageId, type ContributionItem } from "./extensions";

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
  // M4a parts (WP104).
  "plugin-actions",
  "plugin-activity",
  "plugin-badge",
  "plugin-badges",
  "plugin-banner",
  "plugin-banners",
  "plugin-block",
  "plugin-composer",
  "plugin-extension-menu",
  "plugin-preview",
  "plugin-veto",
];

/** The permissions without an argument (`PluginPermission::FIXED`). */
export const FIXED_PERMISSIONS: readonly PluginPermission[] = [
  "mail.read",
  "mail.read.body",
  "calendar.read",
  "ui.dialogs",
  "notifications",
  "storage",
  "mail.organize",
  "mail.send",
  "calendar.write",
  "contacts.write",
  "tasks.write",
  "hooks.arrival",
  "hooks.beforeSend",
  "hooks.open",
  "composer",
  "attachments",
  "background",
  "mcp.expose",
];

/** The Fluent id of a permission's label (`PluginPermission::label`). */
export function permissionMessageId(permission: PluginPermission): string {
  return permission.startsWith("network:")
    ? "plugin-permission-network"
    : `plugin-permission-${permission
        .replaceAll(".", "-")
        .replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}`;
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
  "missing-required-permission",
  "contribution-needs-permission",
  "invalid-hook",
  "duplicate-hook",
  "invalid-extension",
  "duplicate-extension",
  "invalid-job",
  "duplicate-job",
  "invalid-mcp-tool",
  "duplicate-mcp-tool",
];
const HOOKS: readonly PluginHookKind[] = ["arrival", "beforeSend", "open"];
const JOB_OUTCOMES: readonly PluginJobOutcome[] = [
  "ok",
  "error",
  "timeout",
  "cpu-quota",
  "overlap",
];
const CONTRIBUTIONS: readonly ContributionItem["kind"][] = [
  "command",
  "panel",
  "hook",
  "composerAction",
  "markdownBlock",
  "attachmentPreview",
  "attachmentAction",
  "linkAction",
  "job",
  "mcpTool",
];
const EDIT_OPS: readonly ComposerEditOp[] = ["insert", "replace-selection", "replace-all"];

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
    ...["permissions", "confirmations", "activity", "settings", "log", "layout"].map(
      (s) => `plugin-manager-show-${s}`,
    ),
    ...HOOKS.map(hookMessageId),
    ...JOB_OUTCOMES.map((o) => `plugin-job-outcome-${o}`),
    ...CONTRIBUTIONS.map(contributionMessageId),
    ...EDIT_OPS.map((op) => `plugin-composer-edited-${op}`),
    // Layout labels for the integrator's default layout (`@plugin.…`).
    "plugin-manager-tab",
    "plugin-action-install",
    "plugin-action-load-dev",
    "plugin-action-manage",
  ];
}
