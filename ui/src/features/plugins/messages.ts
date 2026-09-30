/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/plugins.ftl` (or, for texts
 * the core builds, `locales/en/core-plugins.ftl`); ids built at runtime are
 * listed by `runtimeIds`.
 */
import type { PluginPermission } from "../../api/generated";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["plugin-"];

/** Literals with those prefixes that are not message ids (part names, widget kinds). */
export const notMessages: readonly string[] = ["plugin-panel"];

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

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  const network: PluginPermission = "network:example.com";
  return [...FIXED_PERMISSIONS, network].map(permissionMessageId);
}
