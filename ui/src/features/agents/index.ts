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
export { requestConfirmation } from "./mock";
export { seedAgentsDemo } from "./mockSeed";
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
  [OPEN_CENTER_ACTION]: { run: (ctx) => ctx.store.getState().openWindow(CENTER_DIALOG) },
};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {
  [CENTER_DIALOG]: ConfirmationCenterDialog,
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
