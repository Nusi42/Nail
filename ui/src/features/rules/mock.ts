/**
 * The mock core's answers to the rules requests (WP22), used by
 * `MockTransport` (`../../api/mock.ts` routes exactly these requests here).
 * State lives in `data.features.rules` ([`createRulesMockState`]).
 */
import type { Request, Response } from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";

/** The requests this feature answers. */
export const RULES_REQUESTS = [
  "ListRules",
  "SaveRule",
  "DeleteRule",
  "ReorderRules",
  "TestRule",
  "RunRule",
  "ListRuleRuns",
  "UndoRuleRun",
  "RuleToText",
  "RuleFromText",
] as const satisfies readonly Request["type"][];

export type RulesRequest = Extract<Request, { type: (typeof RULES_REQUESTS)[number] }>;

export function isRulesRequest(request: Request): request is RulesRequest {
  return (RULES_REQUESTS as readonly string[]).includes(request.type);
}

/** Mock-core state of this feature. */
export type RulesMockState = Record<string, never>;

export function createRulesMockState(): RulesMockState {
  return {};
}

export function answerRules(core: MockCore, request: RulesRequest): Response {
  void core;
  return errorResponse("unsupported", `${request.type} arrives with M1b WP22`);
}
