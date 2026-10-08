/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/plugins.ftl` (or, for texts
 * the core builds, `locales/en/core-plugins.ftl`); ids built at runtime are
 * listed by `runtimeIds`.
 */
import {
  COMPOSER_EDIT_OP_VALUES,
  PLUGIN_HOOK_KIND_VALUES,
  PLUGIN_JOB_OUTCOME_VALUES,
  PLUGIN_LOG_LEVEL_VALUES,
  PLUGIN_LOG_SOURCE_VALUES,
  PLUGIN_MANIFEST_ISSUE_KIND_VALUES,
  PLUGIN_PERMISSIONS,
  PLUGIN_RUN_STATE_VALUES,
  PLUGIN_STOP_REASON_VALUES,
} from "../../api/generated";
import type { PluginPermission } from "../../api/generated";
import { contributionMessageId, hookMessageId, type ContributionItem } from "./extensions";
import { SOURCE_KINDS as INSTALL_SOURCE_KINDS } from "./InstallDialog";
import { MANAGER_SECTIONS } from "./PluginManager";
import { FIELD_ISSUES } from "./tree/values";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["plugin-"];

/** Literals with those prefixes that are not message ids (part, widget, dialog names). */
export const notMessages: readonly string[] = [
  "plugin-panel",
  "plugin-manager",
  "plugin-install",
  "plugin-dialog",
  // M5c window kinds of plug-in dialogs and the send veto.
  "plugin-modal",
  "plugin-send-veto",
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

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  const network: PluginPermission = "network:example.com";
  return [
    ...[...PLUGIN_PERMISSIONS, network].map(permissionMessageId),
    "window-plugin-install",
    "window-plugin-manager",
    ...PLUGIN_RUN_STATE_VALUES.map((s) => `plugin-state-${s}`),
    ...PLUGIN_LOG_LEVEL_VALUES.map((l) => `plugin-log-level-${l}`),
    ...PLUGIN_LOG_SOURCE_VALUES.map((s) => `plugin-log-source-${s}`),
    ...PLUGIN_STOP_REASON_VALUES.map((s) => `plugin-stopped-${s}`),
    ...PLUGIN_MANIFEST_ISSUE_KIND_VALUES.map((k) => `plugin-manifest-${k}`),
    ...FIELD_ISSUES.map((i) => `plugin-field-${i}`),
    ...INSTALL_SOURCE_KINDS.flatMap((k) => [
      `plugin-install-source-${k}`,
      `plugin-install-location-${k}`,
    ]),
    ...MANAGER_SECTIONS.map((s) => `plugin-manager-show-${s}`),
    ...PLUGIN_HOOK_KIND_VALUES.map(hookMessageId),
    ...PLUGIN_JOB_OUTCOME_VALUES.map((o) => `plugin-job-outcome-${o}`),
    ...CONTRIBUTIONS.map(contributionMessageId),
    ...COMPOSER_EDIT_OP_VALUES.map((op) => `plugin-composer-edited-${op}`),
    // Layout labels for the integrator's default layout (`@plugin.…`).
    "plugin-manager-tab",
    "plugin-action-install",
    "plugin-action-load-dev",
    "plugin-action-manage",
  ];
}
