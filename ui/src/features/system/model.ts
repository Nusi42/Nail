/**
 * The M4b desktop contract as the UI sees it (mirrors `nail_api::desktop` /
 * `nail_api::mapi`; ADR 0087): lists of the enum values and the Fluent ids
 * the contract names (texts in `locales/en/core-desktop.ftl`).
 */
import type {
  DefaultAppState,
  DefaultAppTarget,
  InstallKind,
  InstallScope,
  MapiResult,
  UpdateChannel,
  UpdateDisabledReason,
  UpdateFailure,
  UpdateState,
} from "../../api/generated";

export const INSTALL_KINDS = [
  "nsis",
  "msi",
  "portable",
  "development",
] as const satisfies readonly InstallKind[];
export const INSTALL_SCOPES = [
  "per-user",
  "per-machine",
] as const satisfies readonly InstallScope[];
export const UPDATE_CHANNELS = ["stable", "beta"] as const satisfies readonly UpdateChannel[];
export const UPDATE_DISABLED_REASONS = [
  "msi-managed",
  "policy",
  "portable",
  "development",
  "not-configured",
] as const satisfies readonly UpdateDisabledReason[];
export const UPDATE_FAILURES = [
  "network",
  "manifest",
  "signature",
  "download",
  "install",
] as const satisfies readonly UpdateFailure[];
/** In the order the UI lists them (`DefaultAppTarget::ALL`). */
export const DEFAULT_APP_TARGETS = [
  "mailto",
  "webcal",
  "webcals",
  "eml",
  "ics",
  "vcf",
] as const satisfies readonly DefaultAppTarget[];
export const DEFAULT_APP_STATES = [
  "default",
  "other",
  "unknown",
] as const satisfies readonly DefaultAppState[];
export const MAPI_RESULTS = [
  "success",
  "user-abort",
  "failure",
  "login-failure",
  "insufficient-memory",
  "access-denied",
  "too-many-files",
  "too-many-recipients",
  "attachment-not-found",
  "attachment-open-failure",
  "unknown-recipient",
  "bad-recip-type",
  "text-too-large",
  "invalid-recips",
  "not-supported",
  "attachment-too-large",
] as const satisfies readonly MapiResult[];

export const installKindMessageId = (kind: InstallKind) => `install-kind-${kind}`;
export const installScopeMessageId = (scope: InstallScope) => `install-scope-${scope}`;
export const channelMessageId = (channel: UpdateChannel) => `update-channel-${channel}`;
export const disabledReasonMessageId = (reason: UpdateDisabledReason) =>
  `update-disabled-${reason}`;
export const failureMessageId = (failure: UpdateFailure) => `update-failure-${failure}`;
export const defaultAppMessageId = (target: DefaultAppTarget) => `default-app-${target}`;
export const defaultAppStateMessageId = (state: DefaultAppState) => `default-app-state-${state}`;
export const mapiResultMessageId = (result: MapiResult) => `mapi-result-${result}`;

/** Whether "Check now" applies (`UpdateState::can_check`). */
export function canCheck(state: UpdateState): boolean {
  return ["idle", "up-to-date", "available", "ready-to-install", "failed"].includes(state.state);
}

/** Whether "Download" applies (`UpdateState::can_download`). */
export function canDownload(state: UpdateState): boolean {
  return state.state === "available";
}

/** Whether "Restart to install" applies (`UpdateState::can_install`). */
export function canInstall(state: UpdateState): boolean {
  return state.state === "ready-to-install";
}
