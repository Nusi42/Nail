/**
 * The mock core's answers to the M3c Exchange requests (ADR 0072), used by
 * `MockTransport` (`../../api/mock.ts` routes exactly these requests here).
 *
 * Mirrors the core at the contract stage: every Exchange request answers
 * `unsupported` until the integration (WP95) wires the engines. WP94 turns
 * this into a working in-memory Exchange account (mailboxes, categories,
 * automatic replies, rules, rooms) on `data.features.exchange` for its UI
 * tests.
 */
import type {
  AutomaticReplies,
  ExchangeCategory,
  ExchangeMailbox,
  ExchangeRulesStatus,
  Request,
  Response,
  Room,
  RoomList,
} from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";

/** The requests this feature answers. */
export const EXCHANGE_REQUESTS = [
  "GetExchangeStatus",
  "ListExchangeMailboxes",
  "AddExchangeMailbox",
  "UpdateExchangeMailbox",
  "RemoveExchangeMailbox",
  "ListExchangeCategories",
  "CreateExchangeCategory",
  "UpdateExchangeCategory",
  "DeleteExchangeCategory",
  "ClassifyFocused",
  "GetAutomaticReplies",
  "SetAutomaticReplies",
  "GetExchangeRules",
  "SyncExchangeRules",
  "SetExchangeInboxRuleEnabled",
  "DeleteExchangeInboxRule",
  "ListRoomLists",
  "FindRooms",
] as const satisfies readonly Request["type"][];

export type ExchangeRequest = Extract<Request, { type: (typeof EXCHANGE_REQUESTS)[number] }>;

export function isExchangeRequest(request: Request): request is ExchangeRequest {
  return (EXCHANGE_REQUESTS as readonly string[]).includes(request.type);
}

/** The mock's Exchange data by account id (WP94 fills and uses it). */
export interface ExchangeMockState {
  mailboxes: Record<string, ExchangeMailbox[]>;
  categories: Record<string, ExchangeCategory[]>;
  automaticReplies: Record<string, AutomaticReplies>;
  rules: Record<string, ExchangeRulesStatus>;
  roomLists: Record<string, RoomList[]>;
  rooms: Record<string, Room[]>;
}

export function createExchangeMockState(): ExchangeMockState {
  return {
    mailboxes: {},
    categories: {},
    automaticReplies: {},
    rules: {},
    roomLists: {},
    rooms: {},
  };
}

/** Answers like the core at the contract stage (`unsupported`, ADR 0072). */
export function answerExchange(_core: MockCore, request: ExchangeRequest): Response {
  return errorResponse("unsupported", `${request.type} is not wired yet (M3c WP95)`);
}
