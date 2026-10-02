/**
 * Feature slot of M4a WP104 (contract ADR 0079): the confirmation center —
 * writes by AI agents (MCP clients) and plug-ins waiting for the user,
 * with attribution, summary and before / after, approve / deny / "always
 * allow" for non-destructive kinds, the history, and the per-client /
 * per-plug-in action policies (shown by the MCP client settings and the
 * plug-in manager).
 *
 * Only WP104 edits the files in this directory (plus
 * `locales/{en,de}/agents.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`.
 */
import type { ActionActor, Event as NailEvent } from "../../api/generated";
import type { Feature, FeatureEventHandler } from "../types";
import { actorKey } from "./model";

export * from "./model";
export { requestConfirmation } from "./mock";

/** Query keys of this feature. */
export const agentsKeys = {
  all: ["agents"] as const,
  confirmations: ["agents", "confirmations"] as const,
  policies: (actor: ActionActor) => ["agents", "policies", actorKey(actor)] as const,
};

/** The core's events that change what this feature shows. */
export const AGENTS_EVENTS = [
  "ConfirmationRequested",
  "ConfirmationResolved",
  "ActionPoliciesChanged",
] as const satisfies readonly NailEvent["type"][];

/** Layout widget kinds → components (the confirmation center: WP104). */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};

/** Confirmations or policies changed: refetch them. */
export const handleAgentsEvent: FeatureEventHandler = (event, { queryClient }) => {
  if (event.type === "ActionPoliciesChanged") {
    void queryClient.invalidateQueries({ queryKey: agentsKeys.policies(event.actor) });
  } else if (event.type === "ConfirmationRequested" || event.type === "ConfirmationResolved") {
    void queryClient.invalidateQueries({ queryKey: agentsKeys.confirmations });
  }
};

/** Handlers for the core's events. */
export const events: Feature["events"] = [handleAgentsEvent];
/** Background components. */
export const effects: Feature["effects"] = [];
