/**
 * The contract's rules for writes by agents and plug-ins, mirrored from
 * `nail_api::agents` (ADR 0079) for the mock core and the confirmation
 * center: which kinds are destructive, which ask by default, and the
 * policy table of an actor.
 */
import type {
  ActionActor,
  ActionKind,
  ActionPolicy,
  ActionPolicyMode,
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
