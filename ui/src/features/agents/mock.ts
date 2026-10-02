/**
 * The mock core's answers to the M4a confirmation and policy requests
 * (ADR 0079), used by `MockTransport` (`../../api/mock.ts` routes exactly
 * these requests here).
 *
 * Like the core at the contract stage: confirmations live in
 * `data.features.agents.confirmations` (tests and `requestConfirmation`
 * add pending ones, as an MCP client or plug-in would); deciding one
 * records the decision (with "always allow" setting the actor's policy)
 * and emits `ConfirmationResolved` / `ActionPoliciesChanged`; policies are
 * validated with the contract's rules and issue ids. Actors must exist
 * (MCP clients of the mcp mock, plug-ins of the plugins mock).
 */
import type {
  ActionActor,
  ActionKind,
  ActionPolicyMode,
  Confirmation,
  Request,
  Response,
} from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";
import { actorKey, canAlwaysAllow, policyTable } from "./model";

/** The requests this feature answers. */
export const AGENTS_REQUESTS = [
  "ListConfirmations",
  "DecideConfirmation",
  "ListActionPolicies",
  "SetActionPolicy",
] as const satisfies readonly Request["type"][];

export type AgentsRequest = Extract<Request, { type: (typeof AGENTS_REQUESTS)[number] }>;

export function isAgentsRequest(request: Request): request is AgentsRequest {
  return (AGENTS_REQUESTS as readonly string[]).includes(request.type);
}

/** Mock-core state of this feature. */
export interface AgentsMockState {
  /** Every confirmation, oldest first. */
  confirmations: Confirmation[];
  /** Stored policies by actor key (`actorKey`). */
  policies: Record<string, Partial<Record<ActionKind, ActionPolicyMode>>>;
  /** Next confirmation number. */
  nextId: number;
}

export function createAgentsMockState(): AgentsMockState {
  return { confirmations: [], policies: {}, nextId: 1 };
}

const DESTRUCTIVE_ISSUE = "action-policy-issue-destructive";

function destructiveError(kind: ActionKind): Response {
  return {
    type: "Error",
    code: "invalid_request",
    message: `${kind} cannot be allowed without asking`,
    localized: { id: DESTRUCTIVE_ISSUE, args: {} },
  };
}

function actorExists(core: MockCore, actor: ActionActor): boolean {
  return actor.kind === "plugin"
    ? core.data.features.plugins.plugins.some((p) => p.id === actor.plugin_id)
    : core.data.features.mcp.clients.some((c) => c.id === actor.client_id);
}

/**
 * Adds a pending confirmation as a requester would (tests, demos) and
 * emits `ConfirmationRequested`. Missing fields get sensible values.
 */
export function requestConfirmation(
  core: MockCore,
  fields: Pick<Confirmation, "requested_by" | "kind" | "operation" | "summary"> &
    Partial<Confirmation>,
): Confirmation {
  const state = core.data.features.agents;
  const now = new Date();
  const confirmation: Confirmation = {
    id: String(state.nextId++),
    targets: [],
    more_targets: 0,
    changes: [],
    account_ids: [],
    created_at: now.toISOString(),
    expires_at: new Date(now.getTime() + 300_000).toISOString(),
    state: "pending",
    decided_at: null,
    decided_via: null,
    can_always_allow: canAlwaysAllow(fields.kind),
    ...fields,
  };
  state.confirmations.push(confirmation);
  core.emit({ type: "ConfirmationRequested", confirmation: structuredClone(confirmation) });
  return confirmation;
}

export function answerAgents(core: MockCore, request: AgentsRequest): Response {
  const state = core.data.features.agents;
  switch (request.type) {
    case "ListConfirmations": {
      const before = request.before === null ? Infinity : Number(request.before);
      const actor = request.actor === null ? null : actorKey(request.actor);
      let list = state.confirmations.filter(
        (c) =>
          Number(c.id) < before && (actor === null || actorKey(c.requested_by.actor) === actor),
      );
      if (request.filter === "pending") list = list.filter((c) => c.state === "pending");
      else {
        if (request.filter === "decided") list = list.filter((c) => c.state !== "pending");
        list = [...list].reverse();
      }
      const limit = Math.max(1, Math.min(request.limit, 500));
      return { type: "Confirmations", confirmations: structuredClone(list.slice(0, limit)) };
    }
    case "DecideConfirmation": {
      const confirmation = state.confirmations.find((c) => c.id === request.confirmation_id);
      if (!confirmation) {
        return errorResponse("not_found", `confirmation ${request.confirmation_id} not found`);
      }
      const approve = request.decision === "approve";
      if (approve && request.always_allow && !canAlwaysAllow(confirmation.kind)) {
        return destructiveError(confirmation.kind);
      }
      if (confirmation.state !== "pending") {
        return errorResponse(
          "conflict",
          `confirmation ${confirmation.id} is ${confirmation.state}`,
        );
      }
      confirmation.state = approve ? "approved" : "denied";
      confirmation.decided_at = new Date().toISOString();
      confirmation.decided_via = "nail";
      if (approve && request.always_allow) {
        const actor = confirmation.requested_by.actor;
        const key = actorKey(actor);
        state.policies[key] = { ...state.policies[key], [confirmation.kind]: "allow" };
        core.emit({ type: "ActionPoliciesChanged", actor: structuredClone(actor) });
      }
      core.emit({ type: "ConfirmationResolved", confirmation: structuredClone(confirmation) });
      return { type: "ConfirmationDecided", confirmation: structuredClone(confirmation) };
    }
    case "ListActionPolicies": {
      if (!actorExists(core, request.actor)) {
        return errorResponse("not_found", `${actorKey(request.actor)} not found`);
      }
      return {
        type: "ActionPolicies",
        actor: structuredClone(request.actor),
        policies: policyTable(state.policies[actorKey(request.actor)] ?? {}),
      };
    }
    case "SetActionPolicy": {
      if (request.mode === "allow" && !canAlwaysAllow(request.kind)) {
        return destructiveError(request.kind);
      }
      if (!actorExists(core, request.actor)) {
        return errorResponse("not_found", `${actorKey(request.actor)} not found`);
      }
      const key = actorKey(request.actor);
      const kept = Object.entries(state.policies[key] ?? {}).filter(([k]) => k !== request.kind);
      const stored: Partial<Record<ActionKind, ActionPolicyMode>> = Object.fromEntries(
        request.mode === null ? kept : [...kept, [request.kind, request.mode]],
      );
      state.policies[key] = stored;
      core.emit({ type: "ActionPoliciesChanged", actor: structuredClone(request.actor) });
      return {
        type: "ActionPolicies",
        actor: structuredClone(request.actor),
        policies: policyTable(stored),
      };
    }
  }
}
