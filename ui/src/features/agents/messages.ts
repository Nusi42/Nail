/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/agents.ftl` (texts the core
 * builds — kinds, states, summaries, fields — are in
 * `locales/en/core-agents.ftl`); ids built at runtime are listed by
 * `runtimeIds`.
 */
import {
  ACTION_KINDS,
  ACTION_TARGET_KINDS,
  CONFIRMATION_SOURCES,
  CONFIRMATION_STATES,
  kindMessageId,
  sourceMessageId,
  stateMessageId,
  targetMessageId,
} from "./model";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["agents-"];

/** Literals with those prefixes that are not message ids (part, widget, dialog names). */
export const notMessages: readonly string[] = [];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    "window-confirmation-center",
    "agents-center-title",
    "agents-center-empty",
    "agents-actor-plugin",
    "agents-actor-mcp-client",
    "agents-policy-ask",
    "agents-policy-allow",
    "action-policy-issue-destructive",
    // Layout labels for the integrator's default layout (`@agents.…`).
    "agents-center-tab",
    "agents-action-open-center",
    ...ACTION_KINDS.map(kindMessageId),
    ...CONFIRMATION_STATES.map(stateMessageId),
    ...ACTION_TARGET_KINDS.map(targetMessageId),
    ...CONFIRMATION_SOURCES.map(sourceMessageId),
  ];
}
