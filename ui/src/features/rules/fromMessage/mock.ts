/**
 * The mock core's `GetMessageFacts` (R2, ADR 0222): a message's values from
 * its summary and the mock body's headers. The mock keeps no raw headers:
 * the header names and the `List-Id` come from the same helpers as the
 * mock's `SuggestValues` (`../../../api/mockValues.ts`), so the pickers and
 * "values from this mail" agree like the core's.
 */
import type { MessageFacts, MessageId, Response } from "../../../api/generated";
import { errorResponse, type MockCore } from "../../../api/mockMail";
import { mockHeaderNamesOf, mockListOf } from "../../../api/mockValues";

/** `GetMessageFacts` against the mock mailbox. */
export function answerMessageFacts(core: MockCore, messageId: MessageId): Response {
  const d = core.data;
  const message = d.messages.find((m) => m.id === messageId);
  if (!message) return errorResponse("not_found", `message ${messageId} not found`);
  const headers = d.bodies.get(messageId)?.headers;
  const facts: MessageFacts = {
    message_id: message.id,
    account_id: message.account_id,
    folder_id: message.folder_id,
    subject: message.subject,
    from: message.from ? [message.from] : [],
    sender: [],
    to: message.to,
    cc: headers?.cc ?? [],
    reply_to: headers?.reply_to ?? [],
    list_id: mockListOf(core, message)?.id ?? null,
    header_names: mockHeaderNamesOf(core, message),
    tags: [...message.tags],
  };
  return { type: "MessageFacts", facts };
}
