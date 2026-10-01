/**
 * The mock core's answers to the M3b Gmail label requests (ADR 0065), used
 * by `MockTransport` (`../../api/mock.ts` routes exactly these requests
 * here).
 *
 * Mirrors the core at the contract stage: every label request answers
 * `unsupported` until the integration (WP85) wires the Gmail engine. WP84
 * turns this into working in-memory labels (and a demo Gmail account) on
 * `data.features.gmail` for its UI tests.
 */
import type { GmailLabel, Request, Response } from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";

/** The requests this feature answers. */
export const GMAIL_REQUESTS = [
  "ListGmailLabels",
  "CreateGmailLabel",
  "UpdateGmailLabel",
  "DeleteGmailLabel",
] as const satisfies readonly Request["type"][];

export type GmailRequest = Extract<Request, { type: (typeof GMAIL_REQUESTS)[number] }>;

export function isGmailRequest(request: Request): request is GmailRequest {
  return (GMAIL_REQUESTS as readonly string[]).includes(request.type);
}

/** The mock's Gmail data (WP84 fills and uses it). */
export interface GmailMockState {
  /** Labels by account id. */
  labels: Record<string, GmailLabel[]>;
}

export function createGmailMockState(): GmailMockState {
  return { labels: {} };
}

/** Answers like the core at the contract stage (`unsupported`, ADR 0065). */
export function answerGmail(_core: MockCore, request: GmailRequest): Response {
  return errorResponse("unsupported", `${request.type} is not wired yet (M3b WP85)`);
}
