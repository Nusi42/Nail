/**
 * The mock core's answers to the composer requests (WP21), used by
 * `MockTransport` (`../../api/mock.ts` routes exactly these requests here).
 * State lives in `data.features.composer` ([`createComposerMockState`]).
 */
import type { Request, Response } from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";

/** The requests this feature answers. */
export const COMPOSER_REQUESTS = [
  "NewDraft",
  "RenderPreview",
  "ConvertDraft",
  "UploadAttachment",
  "SuggestAddresses",
] as const satisfies readonly Request["type"][];

export type ComposerRequest = Extract<Request, { type: (typeof COMPOSER_REQUESTS)[number] }>;

export function isComposerRequest(request: Request): request is ComposerRequest {
  return (COMPOSER_REQUESTS as readonly string[]).includes(request.type);
}

/** Mock-core state of this feature. */
export type ComposerMockState = Record<string, never>;

export function createComposerMockState(): ComposerMockState {
  return {};
}

export function answerComposer(core: MockCore, request: ComposerRequest): Response {
  void core;
  return errorResponse("unsupported", `${request.type} arrives with M1b WP21`);
}
