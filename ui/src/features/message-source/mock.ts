/**
 * The mock core's answer to `GetMessageSource` (ADR 0194). The mock has no
 * message bytes, so the source is written from the parsed body: the
 * headers it knows, then the text (and the HTML as a second part).
 * `../../api/mock.ts` routes exactly these requests here.
 */
import type { MessageBody, MessageSource, Request, Response } from "../../api/generated";
import type { MockCore } from "../../api/mockMail";
import { formatAddressList } from "../../state/addresses";

/** The requests this feature answers. */
export const SOURCE_REQUESTS = ["GetMessageSource"] as const satisfies readonly Request["type"][];

export type SourceRequest = Extract<Request, { type: (typeof SOURCE_REQUESTS)[number] }>;

export function isSourceRequest(request: Request): request is SourceRequest {
  return (SOURCE_REQUESTS as readonly string[]).includes(request.type);
}

const BOUNDARY = "mock-boundary";

/** A made-up RFC 5322 text of `body` (CRLF line endings). */
export function mockSourceOf(body: MessageBody): string {
  const h = body.headers;
  const header = [
    `From: ${formatAddressList(h.from)}`,
    `To: ${formatAddressList(h.to)}`,
    ...(h.cc.length > 0 ? [`Cc: ${formatAddressList(h.cc)}`] : []),
    `Subject: ${h.subject}`,
    ...(h.date ? [`Date: ${h.date}`] : []),
    ...(h.message_id ? [`Message-ID: <${h.message_id}>`] : []),
    "MIME-Version: 1.0",
  ];
  const text = ["Content-Type: text/plain; charset=utf-8", "", body.text];
  const lines =
    body.sanitized_html === null
      ? [...header, ...text]
      : [
          ...header,
          `Content-Type: multipart/alternative; boundary="${BOUNDARY}"`,
          "",
          `--${BOUNDARY}`,
          ...text,
          `--${BOUNDARY}`,
          "Content-Type: text/html; charset=utf-8",
          "",
          body.sanitized_html,
          `--${BOUNDARY}--`,
        ];
  return `${lines.join("\r\n")}\r\n`;
}

export function answerSource(core: MockCore, request: SourceRequest): Response {
  const body = core.data.bodies.get(request.message_id);
  if (!body) return { type: "Error", code: "not_found", message: `message ${request.message_id}` };
  const original = mockSourceOf(body);
  const source: MessageSource = {
    size: new TextEncoder().encode(original).length,
    original,
    original_truncated: false,
    decoded: [{ type: "text", text: original.replaceAll("\r\n", "\n") }],
    decoded_truncated: false,
    encrypted: false,
  };
  return { type: "MessageSource", source };
}
