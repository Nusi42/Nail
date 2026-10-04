/**
 * Feature slot of M4a WP104 (contract ADR 0079): the confirmation center —
 * writes by AI agents (MCP clients) and plug-ins waiting for the user,
 * with attribution, summary and before / after, approve / deny / "always
 * allow" for non-destructive kinds, the history, notices while something
 * waits, and the per-client / per-plug-in action policies (`PolicyEditor`,
 * mounted by the MCP client settings and the plug-in manager).
 *
 * Contributions: widgets `confirmation-center` (option `show-history`)
 * and `confirmation-indicator` (option `hide-when-empty`), the dialog
 * `confirmation-center`, the action `agents.open-center`, the effect
 * `ConfirmationNotices`. Theme base CSS: `AGENTS_BASE_CSS`.
 *
 * Only WP104 edits the files in this directory (plus
 * `locales/{en,de}/agents.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`.
 */
import type { Event as NailEvent } from "../../api/generated";
import type { Feature, FeatureEventHandler } from "../types";
import { ConfirmationCenter, ConfirmationCenterDialog } from "./ConfirmationCenter";
import { ConfirmationIndicator } from "./ConfirmationIndicator";
import { ConfirmationNotices } from "./ConfirmationNotices";
import { CENTER_DIALOG, CENTER_WIDGET, INDICATOR_WIDGET, OPEN_CENTER_ACTION } from "./names";
import { agentsKeys } from "./queries";
import { agentsStoreFor } from "./store";

export * from "./model";
export * from "./names";
export { agentsKeys, pendingOf, usePendingConfirmations } from "./queries";
export { agentsStoreFor } from "./store";
export { AGENTS_BASE_CSS } from "./parts";
export { ConfirmationCard } from "./ConfirmationCard";
export { PolicyEditor } from "./PolicyEditor";

/** The core's events that change what this feature shows. */
export const AGENTS_EVENTS = [
  "ConfirmationRequested",
  "ConfirmationResolved",
  "ActionPoliciesChanged",
] as const satisfies readonly NailEvent["type"][];

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  [CENTER_WIDGET]: ConfirmationCenter,
  [INDICATOR_WIDGET]: ConfirmationIndicator,
};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  /** Opens the confirmation center window. */
  [OPEN_CENTER_ACTION]: {
    run: (ctx) => {
      ctx.store.getState().openWindow(CENTER_DIALOG);
    },
  },
};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {
  [CENTER_DIALOG]: ConfirmationCenterDialog,
};

/**
 * The window model of this feature's windows (M5c, ADR 0116): tool windows
 * are non-modal with their default and minimum sizes; only confirmations
 * (`confirmation: true`) are modal. The layout's `window` properties win.
 */
export const windows: NonNullable<Feature["windows"]> = {
  [CENTER_DIALOG]: { size: { width: 900, height: 640 }, min_size: { width: 560, height: 400 } },
};

/** Confirmations or policies changed: refetch them; notices for new requests. */
export const handleAgentsEvent: FeatureEventHandler = (event, { queryClient, store }) => {
  switch (event.type) {
    case "ActionPoliciesChanged":
      void queryClient.invalidateQueries({ queryKey: agentsKeys.policies(event.actor) });
      break;
    case "ConfirmationRequested":
      agentsStoreFor(store).getState().notice(event.confirmation);
      void queryClient.invalidateQueries({ queryKey: agentsKeys.confirmations });
      break;
    case "ConfirmationResolved":
      agentsStoreFor(store).getState().dismiss(event.confirmation.id);
      void queryClient.invalidateQueries({ queryKey: agentsKeys.confirmations });
      break;
  }
};

/** Handlers for the core's events. */
export const events: Feature["events"] = [handleAgentsEvent];
/** Background components: the "something waits" notices. */
export const effects: Feature["effects"] = [ConfirmationNotices];

/**
 * Settings labels per widget kind, for the settings search (M5b WP163, ADR
 * 0109): Fluent ids of the setting labels each widget shows.
 */
export const settings: NonNullable<Feature["settings"]> = {
  [CENTER_WIDGET]: {
    labels: [
      "agents-center-tab",
      "agents-center-title",
      "agents-center-pending-label",
      "agents-center-history-label",
    ],
    keywords: "settings-keywords-confirmation-center",
  },
};
