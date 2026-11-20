/**
 * The mock core's `GetMessageFacts` (R2, ADR 0222): a message's values from
 * its summary and the mock body's headers. The mock keeps no raw headers,
 * so `list_id` is `null` and the header names are those the mock knows.
 */
import type { MessageFacts, MessageId, Response } from "../../../api/generated";
import { errorResponse, type MockCore } from "../../../api/mockMail";

/** `GetMessageFacts` against the mock mailbox. */
export function answerMessageFacts(core: MockCore, messageId: MessageId): Response {
  const d = core.data;
  const message = d.messages.find((m) => m.id === messageId);
  if (!message) return errorResponse("not_found", `message ${messageId} not found`);
  const headers = d.bodies.get(messageId)?.headers;
  const cc = headers?.cc ?? [];
  const replyTo = headers?.reply_to ?? [];
  const named: [string, boolean][] = [
    ["From", message.from !== null],
    ["To", message.to.length > 0],
    ["Cc", cc.length > 0],
    ["Reply-To", replyTo.length > 0],
    ["Subject", true],
    ["Date", true],
    ["Message-ID", headers?.message_id != null],
  ];
  const facts: MessageFacts = {
    message_id: message.id,
    account_id: message.account_id,
    folder_id: message.folder_id,
    subject: message.subject,
    from: message.from ? [message.from] : [],
    sender: [],
    to: message.to,
    cc,
    reply_to: replyTo,
    list_id: null,
    header_names: named.filter(([, present]) => present).map(([name]) => name),
  };
  return { type: "MessageFacts", facts };
}
