/**
 * The mock core's attachments of received mail (M5a WP155):
 * `GetMessageAttachment` (the web UI's download) and `SaveAttachment` (the
 * desktop's save after `PickPath`). The mock has no message bytes, so the
 * content is a short made-up text naming the attachment.
 */
import type { Request, Response } from "./generated";
import type { MockCore } from "./mockMail";

/** What the mock recorded (tests read it). */
export interface AttachmentsMockState {
  /** `message_id#index → path` of every saved attachment, in order. */
  saved: string[];
  /** `message_id#index` of every downloaded attachment, in order. */
  downloaded: string[];
}

export function createAttachmentsMockState(): AttachmentsMockState {
  return { saved: [], downloaded: [] };
}

export const ATTACHMENT_REQUESTS = [
  "GetMessageAttachment",
  "SaveAttachment",
] as const satisfies readonly Request["type"][];

export type AttachmentRequest = Extract<Request, { type: (typeof ATTACHMENT_REQUESTS)[number] }>;

export function isAttachmentRequest(request: Request): request is AttachmentRequest {
  return (ATTACHMENT_REQUESTS as readonly string[]).includes(request.type);
}

/** The made-up content of an attachment (base64 of a short text). */
export function mockAttachmentContent(name: string): string {
  return btoa(`Mock content of ${name}`);
}

export function answerAttachment(core: MockCore, request: AttachmentRequest): Response {
  const body = core.data.bodies.get(request.message_id);
  if (!body) {
    return { type: "Error", code: "not_found", message: `message ${request.message_id}` };
  }
  const attachment = body.attachments.find((a) => a.index === request.index);
  if (!attachment) {
    return { type: "Error", code: "not_found", message: `attachment ${String(request.index)}` };
  }
  const state = core.data.features.attachments;
  const key = `${request.message_id}#${String(request.index)}`;
  switch (request.type) {
    case "GetMessageAttachment":
      state.downloaded.push(key);
      return {
        type: "AttachmentData",
        mime: attachment.mime,
        data: mockAttachmentContent(attachment.filename ?? key),
      };
    case "SaveAttachment":
      state.saved.push(`${key} → ${request.path}`);
      return { type: "Done" };
  }
}
