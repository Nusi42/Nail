/**
 * The contract's rules for writes by agents and plug-ins (`nail_api::agents`,
 * ADR 0079) for the mock core and the confirmation center: the per-kind
 * rules come generated (`ACTION_KIND_RULES`), the policy table is a port
 * pinned by `crates/nail-api/tests/ts_vectors.rs` (vectors run in
 * `model.test.ts`).
 */
import {
  ACTION_KIND_MESSAGE_IDS,
  ACTION_KIND_RULES,
  ACTION_KIND_VALUES,
  ACTION_TARGET_KIND_VALUES,
  type ActionActor,
  type ActionAttribution,
  type ActionKind,
  type ActionPolicy,
  type ActionPolicyMode,
  type ActionTargetKind,
  CONFIRMATION_SOURCE_VALUES,
  CONFIRMATION_STATE_MESSAGE_IDS,
  type ConfirmationSource,
  type ConfirmationState,
} from "../../api/generated";

export {
  ACTION_KIND_VALUES as ACTION_KINDS,
  CONFIRMATION_STATE_VALUES as CONFIRMATION_STATES,
} from "../../api/generated";

/** Kinds that remove data: they always ask (`ActionKind::is_destructive`). */
export function isDestructive(kind: ActionKind): boolean {
  return ACTION_KIND_RULES[kind].destructive;
}

/** Kinds that send mail (`ActionKind::is_outgoing`). */
export function isOutgoing(kind: ActionKind): boolean {
  return ACTION_KIND_RULES[kind].outgoing;
}

/** Whether "always allow" may be set (`ActionKind::can_always_allow`). */
export function canAlwaysAllow(kind: ActionKind): boolean {
  return ACTION_KIND_RULES[kind].can_always_allow;
}

/** The policy until the user changes it (`ActionKind::default_mode`). */
export function defaultMode(kind: ActionKind): ActionPolicyMode {
  return ACTION_KIND_RULES[kind].default_mode;
}

/** The full table of an actor from its stored modes (`ActionPolicy::table`). */
export function policyTable(stored: Partial<Record<ActionKind, ActionPolicyMode>>): ActionPolicy[] {
  return ACTION_KIND_VALUES.map((kind) => {
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
  return ACTION_KIND_MESSAGE_IDS[kind];
}

/** Fluent id of a state's name (in `core-agents.ftl`). */
export function stateMessageId(state: ConfirmationState): string {
  return CONFIRMATION_STATE_MESSAGE_IDS[state];
}

/** Every kind of affected item (`ActionTargetKind`). */
export const ACTION_TARGET_KINDS: readonly ActionTargetKind[] = ACTION_TARGET_KIND_VALUES;

/** Where a decision can come from (`ConfirmationSource`). */
export const CONFIRMATION_SOURCES: readonly ConfirmationSource[] = CONFIRMATION_SOURCE_VALUES;

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
