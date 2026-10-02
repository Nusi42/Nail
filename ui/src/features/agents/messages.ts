/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/agents.ftl` (texts the core
 * builds — kinds, states, summaries — are in `locales/en/core-agents.ftl`);
 * ids built at runtime are listed by `runtimeIds`.
 */
import { ACTION_KINDS, CONFIRMATION_STATES, kindMessageId, stateMessageId } from "./model";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["agents-"];

/** Literals with those prefixes that are not message ids (part, widget, dialog names). */
export const notMessages: readonly string[] = [];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    "agents-center-title",
    "agents-center-empty",
    "action-policy-issue-destructive",
    ...ACTION_KINDS.map(kindMessageId),
    ...CONFIRMATION_STATES.map(stateMessageId),
  ];
}
