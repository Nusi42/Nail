/**
 * Feature slot of M4b WP114 (contract ADR 0087): the Windows desktop app's
 * own settings and screens — updates and "About" (version, channel, check
 * now, release notes, download / restart-to-install, policy-disabled
 * state), tray and startup settings, the `nail plugin dev` endpoint switch,
 * default apps (status and the button to Settings → Default apps), the
 * first-run screen and the composer entry for mail from other programs
 * (Simple MAPI "Send to", `LaunchRequest::ComposeDraft`).
 *
 * Only WP114 edits the files in this directory (plus
 * `locales/{en,de}/system.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`. The contract's
 * texts are in `locales/en/core-desktop.ftl`.
 */
import type { Event as NailEvent } from "../../api/generated";
import type { Feature, FeatureEventHandler } from "../types";

export * from "./model";
export { setUpdateState, updateStatusOf } from "./mock";

/** Query keys of this feature. */
export const systemKeys = {
  all: ["system"] as const,
  installation: ["system", "installation"] as const,
  updates: ["system", "updates"] as const,
  desktop: ["system", "desktop"] as const,
  defaultApps: ["system", "default-apps"] as const,
  firstRun: ["system", "first-run"] as const,
};

/** The core's events that change what this feature shows. */
export const SYSTEM_EVENTS = [
  "UpdateStatusChanged",
  "DesktopSettingsChanged",
  "DefaultAppsChanged",
] as const satisfies readonly NailEvent["type"][];

/** Layout widget kinds → components (WP114). */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};

/** Keeps the queries fresh: the update status arrives with its event. */
export const handleSystemEvent: FeatureEventHandler = (event, { queryClient }) => {
  switch (event.type) {
    case "UpdateStatusChanged":
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
/** Background components. */
export const effects: Feature["effects"] = [];
