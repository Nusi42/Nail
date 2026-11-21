/**
 * The mock core's `SuggestValues` (R2 shared pickers, ADR 0222): tags in
 * use on the mock's messages, most used first. The mock keeps no raw
 * headers: a message's header names are those its summary and mock body
 * show (`mockHeaderNamesOf`), and one mailing list, the mailing-list post
 * of the mock data (`LIST_MESSAGE_ID`), has a `List-Id` derived from the
 * list address the post was sent to (`rust-announce@lists.example` →
 * `rust-announce.lists.example`, named after the sender like a `List-Id`
 * phrase; `mockListOf`). `GetMessageFacts` of the mock
 * (`../features/rules/fromMessage/mock.ts`) reads the same helpers.
 */
import type { MessageSummary, Request, Response, ValueSuggestion } from "./generated";
import { LIST_MESSAGE_ID } from "./mockData";
import type { MockCore } from "./mockMail";

type SuggestValuesRequest = Extract<Request, { type: "SuggestValues" }>;

export function isValuesRequest(request: Request): request is SuggestValuesRequest {
  return request.type === "SuggestValues";
}

/**
 * Counts values case-insensitively (first spelling wins), most used first;
 * `labels` names values (a list's name), keyed by the lower-case value.
 */
export function rankValues(
  values: Iterable<string>,
  prefix: string,
  limit: number,
  labels: ReadonlyMap<string, string> = new Map(),
): ValueSuggestion[] {
  const needle = prefix.trim().toLowerCase();
  const counted = new Map<string, ValueSuggestion>();
  for (const value of values) {
    const key = value.toLowerCase();
    if (!key.startsWith(needle)) continue;
    const entry = counted.get(key) ?? { value, label: labels.get(key) ?? null, count: 0 };
    entry.count++;
    counted.set(key, entry);
  }
  return [...counted.values()]
    .sort((a, b) => b.count - a.count || a.value.localeCompare(b.value))
    .slice(0, Math.max(0, limit));
}

/** The `List-Id` of a mock message (only the mailing-list post has one) and the list's name. */
export function mockListOf(
  core: MockCore,
  message: MessageSummary,
): { id: string; name: string | null } | null {
  if (core.data.bodies.get(message.id)?.headers.message_id !== LIST_MESSAGE_ID) return null;
  const to = message.to[0]?.email;
  if (to === undefined) return null;
  return { id: to.replace("@", "."), name: message.from?.name ?? null };
}

/**
 * The header names of a mock message, in message order: those its summary
 * and mock body show (the mock keeps no raw headers), and `List-Id` on the
 * mailing-list post.
 */
export function mockHeaderNamesOf(core: MockCore, message: MessageSummary): string[] {
  const headers = core.data.bodies.get(message.id)?.headers;
  const named: [string, boolean][] = [
    ["From", message.from !== null],
    ["To", message.to.length > 0],
    ["Cc", (headers?.cc ?? []).length > 0],
    ["Reply-To", (headers?.reply_to ?? []).length > 0],
    ["Subject", true],
    ["Date", true],
    ["Message-ID", headers?.message_id != null],
    ["List-Id", mockListOf(core, message) !== null],
  ];
  return named.filter(([, present]) => present).map(([name]) => name);
}

export function answerValues(core: MockCore, request: SuggestValuesRequest): Response {
  const { kind, prefix, limit } = request;
  const labels = new Map<string, string>();
  let values: string[] = [];
  if (kind === "tag") values = core.data.messages.flatMap((m) => m.tags);
  else if (kind === "header-name") {
    values = core.data.messages.flatMap((m) => mockHeaderNamesOf(core, m));
  } else {
    for (const message of core.data.messages) {
      const list = mockListOf(core, message);
      if (!list) continue;
      values.push(list.id);
      if (list.name !== null) labels.set(list.id.toLowerCase(), list.name);
    }
  }
  return { type: "ValueSuggestions", values: rankValues(values, prefix, limit, labels) };
}
