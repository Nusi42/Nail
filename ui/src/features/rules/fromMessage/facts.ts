/**
 * "Values from this mail" (R2, ADR 0222): when the rules window was opened
 * from a message (its `origin` in the rules store), a condition's value
 * field offers that message's values — sender address, name and domain,
 * recipients, subject, mailing list, reply-to, account, folder, header
 * names. The values come from the core (`GetMessageFacts`).
 *
 * Contract: `getMessageFacts` / `useMessageFacts` work; `valuesForCondition`
 * is a stub the "values from the selected mail" package (WR5) fills in.
 */
import { useQuery } from "@tanstack/react-query";
import type { NailClient } from "../../../api/client";
import type { Condition, MessageFacts, MessageId } from "../../../api/generated";
import { useClient } from "../../../state/queries";

/** What kind of value a suggestion is (its label in the list). */
export const MESSAGE_VALUE_KINDS = [
  "address",
  "name",
  "domain",
  "subject",
  "mailing-list",
  "account",
  "folder",
  "header-name",
] as const;

export type MessageValueKind = (typeof MESSAGE_VALUE_KINDS)[number];

/** One value of the message a condition field can take. */
export interface MessageValue {
  kind: MessageValueKind;
  /** What the field takes. */
  value: string;
  /** What the list shows (the value, or a name with its address). */
  label: string;
}

export const messageFactsKeys = {
  facts: (messageId: MessageId) => ["message-facts", messageId] as const,
};

/** The values of one message rules can test. */
export async function getMessageFacts(
  client: NailClient,
  messageId: MessageId,
): Promise<MessageFacts> {
  return (await client.request({ type: "GetMessageFacts", message_id: messageId }, "MessageFacts"))
    .facts;
}

/** The values of the message, `null` id = no message (the query stays idle). */
export function useMessageFacts(messageId: MessageId | null) {
  const client = useClient();
  return useQuery({
    queryKey: messageFactsKeys.facts(messageId ?? ""),
    queryFn: () => getMessageFacts(client, messageId ?? ""),
    enabled: messageId !== null,
  });
}

/** The values a condition's field can take from the message (stub: none yet). */
// Contract stub (ADR 0222): WR5 implements it.
export const valuesForCondition: (
  facts: MessageFacts,
  condition: Condition,
) => MessageValue[] = () => [];
