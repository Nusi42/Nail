/**
 * Feature slot of M2b WP43 (remote UI and privacy UI, ADR 0036; ADR 0039
 * for WP43): login (passkey first, password + TOTP), pairing on both
 * sides (the desktop shows the code and the certificate fingerprint, the
 * browser enters the code), users, sessions and devices, remote-UI
 * settings with internet mode shown as locked with its reason, the "load
 * remote content" bar in the reader with per-message / sender / domain
 * choices, the allow-lists, the security audit view and security alerts.
 *
 * Only WP43 edits the files in this directory (plus
 * `crates/nail-config/src/widgets/remote.rs` for widget kinds and
 * `locales/{en,de}/remote.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`. The browser's
 * login page talks to `POST /api/auth` (`AuthRequest` / `AuthResponse`),
 * not to `NailClient`: it runs before there is a session.
 *
 * Components for the app shell and the reader are re-exported at the end.
 */
import type { JsonValue, MessageBody, RemoteContentScope } from "../../api/generated";
import { queryKeys } from "../../state/queries";
import type { Feature, FeatureEventHandler } from "../types";
import { RemoteAlerts } from "./Alerts";
import { remoteAuth } from "./authConfig";
import { CONTENT_SCOPES } from "./model";
import { PAIR_DIALOG, PairDeviceDialog } from "./PairDevice";
import { applyRemoteStatus, remoteKeys } from "./queries";
import { RemoteAuditWidget } from "./RemoteAudit";
import { RemoteContentAllowsWidget, RemoteContentWidget, remoteContentView } from "./RemoteContent";
import { RemoteDevicesWidget } from "./RemoteDevices";
import { RemoteSettingsWidget } from "./RemoteSettings";
import { RemoteUsersWidget } from "./RemoteUsers";
import { remoteStore } from "./state";
import { REAUTH_WINDOW, ReauthWindow } from "./ReauthWindow";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "remote-settings": RemoteSettingsWidget,
  "remote-users": RemoteUsersWidget,
  "remote-devices": RemoteDevicesWidget,
  "remote-audit": RemoteAuditWidget,
  "remote-content": RemoteContentWidget,
  "remote-content-allows": RemoteContentAllowsWidget,
};

function scopeArg(args: readonly JsonValue[]): RemoteContentScope {
  const arg = args[0];
  return typeof arg === "string" && (CONTENT_SCOPES as readonly string[]).includes(arg)
    ? (arg as RemoteContentScope)
    : "message";
}

/** Action ids → handlers (prefix `remote.`). */
export const actions: Feature["actions"] = {
  /** Opens the pairing dialog (desktop). */
  "remote.pair": {
    run: (ctx) => {
      remoteStore.getState().setPairUser(null);
      ctx.store.getState().openWindow(PAIR_DIALOG);
    },
  },
  /**
   * Loads the selected message's remote content: `remote.content.allow`
   * (this message), `remote.content.allow "sender"` or `… "domain"`.
   */
  "remote.content.allow": {
    run: async (ctx, args) => {
      const id = ctx.store.getState().selectedMessageId;
      if (id === null) return;
      await ctx.client.request(
        { type: "AllowRemoteContent", message_id: id, scope: scopeArg(args) },
        "RemoteContentAllowed",
      );
      await ctx.queryClient.invalidateQueries({ queryKey: queryKeys.message(id) });
      await ctx.queryClient.invalidateQueries({ queryKey: remoteKeys.allows });
    },
    enabled: (ctx, args) => {
      const id = ctx.store.getState().selectedMessageId;
      if (id === null) return false;
      const message = ctx.queryClient.getQueryData<MessageBody>(queryKeys.message(id));
      const view = message === undefined ? null : remoteContentView(message);
      if (view === null || !view.blocked) return false;
      const scope = scopeArg(args);
      return scope === "message" || (scope === "sender" ? view.sender : view.domain) !== null;
    },
  },
  /** Signs this browser out of the remote UI. */
  "remote.logout": {
    run: async () => {
      await remoteAuth().transport.send({ type: "Logout" });
      remoteStore.getState().endSession("logout");
    },
    enabled: () => remoteStore.getState().session !== null,
  },
};

/** Named dialogs (opened with `window.open "<name>"`). */
export const dialogs: Feature["dialogs"] = {
  [PAIR_DIALOG]: PairDeviceDialog,
  [REAUTH_WINDOW]: ReauthWindow,
};

/**
 * The window model of this feature's windows (M5c, ADR 0116): tool windows
 * are non-modal with their default and minimum sizes; only confirmations
 * (`confirmation: true`) are modal. The layout's `window` properties win.
 */
export const windows: NonNullable<Feature["windows"]> = {
  [PAIR_DIALOG]: { size: { width: 520, height: 600 }, min_size: { width: 420, height: 480 } },
  // "Confirm it’s you": a modal confirmation (M5c).
  [REAUTH_WINDOW]: {
    confirmation: true,
    resizable: true,
    size: { width: 440, height: 360 },
    min_size: { width: 360, height: 300 },
  },
};

/**
 * Keeps the remote views current: status → cached settings; access
 * changes → users, devices, sessions, codes, audit; a session ended →
 * the lists (and, when it is this browser's, the login page); alerts →
 * a toast; policy → allow-lists and the shown messages.
 */
export const handleRemoteEvent: FeatureEventHandler = (event, { queryClient }) => {
  switch (event.type) {
    case "RemoteUiStatusChanged":
      applyRemoteStatus(queryClient, event.status);
      break;
    case "RemoteAccessChanged":
      void queryClient.invalidateQueries({ queryKey: remoteKeys.access });
      void queryClient.invalidateQueries({ queryKey: remoteKeys.allAudit });
      break;
    case "RemoteSessionEnded": {
      const own = remoteStore.getState().session;
      if (own !== null && own.id === event.session_id) {
        remoteStore.getState().endSession(event.reason);
      }
      void queryClient.invalidateQueries({ queryKey: remoteKeys.access });
      void queryClient.invalidateQueries({ queryKey: remoteKeys.allAudit });
      break;
    }
    case "RemoteSecurityAlert":
      remoteStore.getState().pushAlert(event.alert);
      void queryClient.invalidateQueries({ queryKey: remoteKeys.access });
      void queryClient.invalidateQueries({ queryKey: remoteKeys.allAudit });
      break;
    case "RemoteContentPolicyChanged":
      void queryClient.invalidateQueries({ queryKey: remoteKeys.allows });
      void queryClient.invalidateQueries({ queryKey: ["message"] });
      break;
  }
};

/** Handlers for core events. */
export const events: Feature["events"] = [handleRemoteEvent];

/** Background components mounted once in the app shell (the alert toasts). */
export const effects: Feature["effects"] = [RemoteAlerts];

// ---------------------------------------------------------------------------
// Mount points for the app shell and the reader (ADR 0039).

export { AUTH_PATH, CSRF_HEADER, HttpAuthTransport, type AuthTransport } from "./authClient";
export { configureRemoteAuth, remoteAuth } from "./authConfig";
export { RemoteLogin, RemoteReauth, reauthSecondsLeft } from "./Login";
export { decorateRemoteContent } from "./mock";
export { MockAuthServer } from "./mockAuth";
export { browserPasskeys, type PasskeyApi } from "./passkeys";
export { RemoteContentBar, remoteContentView } from "./RemoteContent";
export { remoteStore, useRemoteState } from "./state";
export { ReauthPrompt, REAUTH_WINDOW, setReauthMethods } from "./ReauthWindow";

/**
 * Settings labels per widget kind, for the settings search (M5b WP163, ADR
 * 0109): Fluent ids of the setting labels each widget shows.
 */
export const settings: NonNullable<Feature["settings"]> = {
  "remote-settings": {
    labels: [
      "remote-settings-tab",
      "remote-settings-title",
      "remote-settings-enabled",
      "remote-status-title",
      "remote-settings-network",
      "remote-settings-bind",
      "remote-settings-port",
      "remote-settings-host-name",
      "remote-settings-networks",
      "remote-settings-sessions",
      "remote-settings-idle",
      "remote-settings-max-hours",
      "remote-settings-reauth",
      "remote-settings-audit-retention",
      "remote-internet-title",
      "remote-internet-mode",
      "remote-fingerprint-label",
    ],
    keywords: "settings-keywords-remote-settings",
  },
  "remote-users": {
    labels: [
      "remote-settings-tab-users",
      "remote-users-title",
      "remote-user-create",
      "remote-passkeys-title",
      "remote-password-title",
      "remote-totp-title",
    ],
    keywords: "settings-keywords-remote-users",
  },
  "remote-devices": {
    labels: [
      "remote-settings-tab-devices",
      "remote-devices-title",
      "remote-pair-open-dialog",
      "remote-pairing-title",
      "remote-sessions-title",
    ],
  },
  "remote-audit": {
    labels: ["remote-settings-tab-audit", "remote-audit-title"],
  },
  "remote-content-allows": {
    labels: ["remote-settings-tab-privacy", "remote-allows-title"],
    keywords: "settings-keywords-remote-content-allows",
  },
};
