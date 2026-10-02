/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/system.ftl` (texts the
 * contract names — installation kinds, update channels / failures / policy
 * reasons, default apps, MAPI results — are in
 * `locales/en/core-desktop.ftl`); ids built at runtime are listed by
 * `runtimeIds`.
 */
import {
  DEFAULT_APP_STATES,
  DEFAULT_APP_TARGETS,
  INSTALL_KINDS,
  INSTALL_SCOPES,
  MAPI_RESULTS,
  UPDATE_CHANNELS,
  UPDATE_DISABLED_REASONS,
  UPDATE_FAILURES,
  channelMessageId,
  defaultAppMessageId,
  defaultAppStateMessageId,
  disabledReasonMessageId,
  failureMessageId,
  installKindMessageId,
  installScopeMessageId,
  mapiResultMessageId,
} from "./model";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["system-"];

/** Literals with those prefixes that are not message ids (part, widget, dialog names). */
export const notMessages: readonly string[] = [];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    "system-settings-title",
    "update-channel-locked",
    "desktop-unavailable",
    "mapi-draft-from-program",
    ...INSTALL_KINDS.map(installKindMessageId),
    ...INSTALL_SCOPES.map(installScopeMessageId),
    ...UPDATE_CHANNELS.map(channelMessageId),
    ...UPDATE_DISABLED_REASONS.map(disabledReasonMessageId),
    ...UPDATE_FAILURES.map(failureMessageId),
    ...DEFAULT_APP_TARGETS.map(defaultAppMessageId),
    ...DEFAULT_APP_STATES.map(defaultAppStateMessageId),
    ...MAPI_RESULTS.map(mapiResultMessageId),
  ];
}
