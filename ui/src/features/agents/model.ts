/**
 * The contract's rules for writes by agents and plug-ins, mirrored from
 * `nail_api::agents` (ADR 0079) for the mock core and the confirmation
 * center: which kinds are destructive, which ask by default, and the
 * policy table of an actor.
 */
import type {
  ActionActor,
  ActionAttribution,
  ActionKind,
  ActionPolicy,
  ActionPolicyMode,
  ActionTargetKind,
  ConfirmationSource,
  ConfirmationState,
} from "../../api/generated";

/** Every kind, in display order (`ActionKind::ALL`). */
export const ACTION_KINDS: readonly ActionKind[] = [
  "organize",
  "trash",
  "permanent-delete",
  "draft",
  "send",
  "calendar",
  "invite",
  "contact",
  "task",
  "pim-delete",
  "rule",
];

/** Every state (`ConfirmationState::ALL`). */
export const CONFIRMATION_STATES: readonly ConfirmationState[] = [
  "pending",
  "approved",
  "denied",
  "expired",
  "cancelled",
  "failed",
];

/** Kinds that remove data: they always ask (`ActionKind::is_destructive`). */
export function isDestructive(kind: ActionKind): boolean {
  return kind === "trash" || kind === "permanent-delete" || kind === "pim-delete";
}

/** Kinds that send mail (`ActionKind::is_outgoing`). */
export function isOutgoing(kind: ActionKind): boolean {
  return kind === "send" || kind === "invite";
}

/** Whether "always allow" may be set (`ActionKind::can_always_allow`). */
export function canAlwaysAllow(kind: ActionKind): boolean {
  return !isDestructive(kind);
}

/** The policy until the user changes it (`ActionKind::default_mode`). */
export function defaultMode(kind: ActionKind): ActionPolicyMode {
  return isDestructive(kind) || isOutgoing(kind) || kind === "rule" ? "ask" : "allow";
}

/** The full table of an actor from its stored modes (`ActionPolicy::table`). */
export function policyTable(stored: Partial<Record<ActionKind, ActionPolicyMode>>): ActionPolicy[] {
  return ACTION_KINDS.map((kind) => {
    const raw = stored[kind];
    const honoured = raw === "ask" || (raw === "allow" && canAlwaysAllow(kind)) ? raw : undefined;
    return {
      kind,
      mode: honoured ?? defaultMode(kind),
      customized: honoured !== undefined,
      can_allow: canAlwaysAllow(kind),
    };
  });
}

/** A stable key of an actor (for maps). */
export function actorKey(actor: ActionActor): string {
  return actor.kind === "plugin" ? `plugin:${actor.plugin_id}` : `${actor.kind}:${actor.client_id}`;
}

/** Fluent id of a kind's name (in `core-agents.ftl`). */
export function kindMessageId(kind: ActionKind): string {
  return `action-kind-${kind}`;
}

/** Fluent id of a state's name (in `core-agents.ftl`). */
export function stateMessageId(state: ConfirmationState): string {
  return `confirmation-state-${state}`;
}

/** Every kind of affected item (`ActionTargetKind`). */
export const ACTION_TARGET_KINDS: readonly ActionTargetKind[] = [
  "message",
  "draft",
  "folder",
  "event",
  "contact",
  "task",
  "rule",
  "recipient",
];

/** Where a decision can come from (`ConfirmationSource`). */
export const CONFIRMATION_SOURCES: readonly ConfirmationSource[] = [
  "nail",
  "elicitation",
  "system",
];

/** Fluent id of an affected item's kind (in `agents.ftl`). */
export function targetMessageId(kind: ActionTargetKind): string {
  return `agents-target-${kind}`;
}

/** Fluent id of where a decision came from (in `agents.ftl`). */
export function sourceMessageId(source: ConfirmationSource): string {
  return `agents-via-${source}`;
}

/**
 * Who asks, as the confirmation center names it: always the kind of actor
 * (AI agent or plug-in) next to its name, so a plug-in cannot pass itself
 * off as Nail or as an agent.
 */
export function attributionMessage(by: ActionAttribution): { id: string; args: { name: string } } {
  return by.actor.kind === "plugin"
    ? { id: "agents-actor-plugin", args: { name: by.name } }
    : { id: "agents-actor-mcp-client", args: { name: by.name } };
}
