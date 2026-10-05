/**
 * Feature slot of M4b WP114 (contract ADR 0087): the Windows desktop app's
 * own settings and screens — updates and "About" (version, channel, check
 * now, release notes, download / restart-to-install, policy-disabled
 * state), tray and startup settings, the `nail plugin dev` endpoint switch,
 * default apps (status and the button to Settings → Default apps), the
 * first-run screen and the composer notice for mail from other programs
 * (Simple MAPI "Send to", `LaunchRequest::ComposeDraft`).
 *
 * Contributions: widgets `update-settings` (option `show-installation`),
 * `desktop-settings` (option `show-dev-endpoint`), `default-apps` (option
 * `show-mapi`); dialogs `about-nail`, `first-run`; actions `system.about`,
 * `system.check-updates`, `system.first-run`, `system.default-apps`; the
 * effect `FirstRunGate`; for other slots `openProgramDraft` (launch) and
 * `ProgramDraftNotice` (composer). Theme base CSS: `SYSTEM_BASE_CSS`.
 *
 * Only WP114 edits the files in this directory (plus
 * `locales/{en,de}/system.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`. The contract's
 * texts are in `locales/en/core-desktop.ftl`.
 */
import type { Event as NailEvent } from "../../api/generated";
import type { Feature, FeatureEventHandler } from "../types";
import { AboutNailDialog } from "./AboutNail";
import { DefaultAppsWidget } from "./DefaultApps";
import { DesktopSettingsWidget } from "./DesktopSettings";
import { FirstRunDialog, FirstRunGate } from "./FirstRun";
import {
  ABOUT_ACTION,
  ABOUT_DIALOG,
  CHECK_UPDATES_ACTION,
  DEFAULT_APPS_ACTION,
  DEFAULT_APPS_WIDGET,
  DESKTOP_SETTINGS_WIDGET,
  FIRST_RUN_ACTION,
  FIRST_RUN_DIALOG,
  UPDATE_SETTINGS_WIDGET,
} from "./names";
import { systemKeys } from "./queries";
import { UpdateSettingsWidget } from "./UpdateSettings";

export * from "./model";
export * from "./names";
export { systemKeys } from "./queries";
export { systemStoreFor } from "./store";
export { SYSTEM_BASE_CSS } from "./parts";
export { AboutNail } from "./AboutNail";
export { ChooseDefaultAppsButton, DefaultAppsContent } from "./DefaultApps";
export { FirstRun, FirstRunGate } from "./FirstRun";
export { openProgramDraft, ProgramDraftNotice, programDraftMessage } from "./ProgramDraft";
export { UpdateSection } from "./UpdateSettings";
export { ReleaseNotes, UpdateStatusPanel } from "./UpdateStatus";

/** The core's events that change what this feature shows. */
export const SYSTEM_EVENTS = [
  "UpdateStatusChanged",
  "DesktopSettingsChanged",
  "DefaultAppsChanged",
] as const satisfies readonly NailEvent["type"][];

/** Layout widget kinds → components (kinds registered in nail-config `widgets/system.rs`). */
export const widgets: Feature["widgets"] = {
  [UPDATE_SETTINGS_WIDGET]: UpdateSettingsWidget,
  [DESKTOP_SETTINGS_WIDGET]: DesktopSettingsWidget,
  [DEFAULT_APPS_WIDGET]: DefaultAppsWidget,
};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  /** Opens "About Nail". */
  [ABOUT_ACTION]: {
    run: (ctx) => {
      ctx.store.getState().openWindow(ABOUT_DIALOG);
    },
  },
  /** Opens the first-run screen again. */
  [FIRST_RUN_ACTION]: {
    run: (ctx) => {
      ctx.store.getState().openWindow(FIRST_RUN_DIALOG);
    },
  },
  /** Checks for updates now (the status arrives with `UpdateStatusChanged`). */
  [CHECK_UPDATES_ACTION]: {
    run: async (ctx) => {
      const answer = await ctx.client.call({ type: "CheckForUpdates" });
      if (answer.type === "UpdateStatus") {
        // Refetched, not set: events may already be newer than the answer.
        await ctx.queryClient.invalidateQueries({ queryKey: systemKeys.updates });
      } else {
        // Disabled, busy or not the desktop app: "About" shows why.
        ctx.store.getState().openWindow(ABOUT_DIALOG);
      }
    },
  },
  /** Opens Windows Settings → Default apps on Nail's page. */
  [DEFAULT_APPS_ACTION]: {
    run: async (ctx) => {
      await ctx.client.call({ type: "OpenDefaultAppsSettings" });
    },
  },
};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {
  [ABOUT_DIALOG]: AboutNailDialog,
  [FIRST_RUN_DIALOG]: FirstRunDialog,
};

/**
 * The window model of this feature's windows (M5c, ADR 0116): tool windows
 * are non-modal with their default and minimum sizes; only confirmations
 * (`confirmation: true`) are modal. The layout's `window` properties win.
 */
export const windows: NonNullable<Feature["windows"]> = {
  [ABOUT_DIALOG]: { size: { width: 520, height: 440 }, min_size: { width: 400, height: 320 } },
  [FIRST_RUN_DIALOG]: { size: { width: 760, height: 600 }, min_size: { width: 560, height: 440 } },
};

/** Keeps the queries fresh: the update status arrives with its event. */
export const handleSystemEvent: FeatureEventHandler = (event, { queryClient }) => {
  switch (event.type) {
    case "UpdateStatusChanged":
      // A fetch still running (the refetch after "Check now") read an older
      // status: it must not land after this one and undo it.
      void queryClient.cancelQueries({ queryKey: systemKeys.updates });
      queryClient.setQueryData(systemKeys.updates, event.status);
      break;
    case "DesktopSettingsChanged":
      void queryClient.invalidateQueries({ queryKey: systemKeys.desktop });
      break;
    case "DefaultAppsChanged":
      void queryClient.invalidateQueries({ queryKey: systemKeys.defaultApps });
      break;
  }
};

/** Handlers for the core's events. */
export const events: Feature["events"] = [handleSystemEvent];
/** Background components: offers the first-run screen once. */
export const effects: Feature["effects"] = [FirstRunGate];

/**
 * Settings labels per widget kind, for the settings search (M5b WP163, ADR
 * 0109): Fluent ids of the setting labels each widget shows.
 */
export const settings: NonNullable<Feature["settings"]> = {
  [UPDATE_SETTINGS_WIDGET]: {
    labels: [
      "system-settings-tab-updates",
      "system-updates-title",
      "system-update-options",
      "system-update-channel",
      "system-update-auto-check",
      "system-update-auto-download",
    ],
    keywords: "settings-keywords-update-settings",
  },
  [DESKTOP_SETTINGS_WIDGET]: {
    labels: [
      "system-settings-tab-desktop",
      "system-desktop-title",
      "system-desktop-tray",
      "system-desktop-close-to-tray",
      "system-desktop-startup",
      "system-desktop-start-with-windows",
      "system-desktop-start-hidden",
      "system-desktop-developer",
      "system-desktop-dev-endpoint",
    ],
    keywords: "settings-keywords-desktop-settings",
  },
  [DEFAULT_APPS_WIDGET]: {
    labels: [
      "system-settings-tab-default-apps",
      "system-default-apps-title",
      "system-default-apps-list",
      "system-default-apps-mapi",
      "system-default-apps-choose",
    ],
    keywords: "settings-keywords-default-apps",
  },
};
