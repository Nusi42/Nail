/**
 * The mock core's `SuggestValues` (R2 shared pickers, ADR 0222): tags in
 * use on the mock's messages, most used first. The mock keeps no raw
 * headers: it knows no mailing lists and only the header names every mock
 * message has.
 */
import type { Request, Response, ValueSuggestion } from "./generated";
import type { MockCore } from "./mockMail";

/** Header names every mock message has (the mock keeps no raw headers). */
export const MOCK_HEADER_NAMES = ["From", "To", "Subject", "Date", "Message-ID"] as const;

type SuggestValuesRequest = Extract<Request, { type: "SuggestValues" }>;

export function isValuesRequest(request: Request): request is SuggestValuesRequest {
  return request.type === "SuggestValues";
}

/** Counts values case-insensitively (first spelling wins), most used first. */
export function rankValues(
  values: Iterable<string>,
  prefix: string,
  limit: number,
): ValueSuggestion[] {
  const needle = prefix.trim().toLowerCase();
  const counted = new Map<string, ValueSuggestion>();
  for (const value of values) {
    const key = value.toLowerCase();
    if (!key.startsWith(needle)) continue;
    const entry = counted.get(key) ?? { value, label: null, count: 0 };
    entry.count++;
    counted.set(key, entry);
  }
  return [...counted.values()]
    .sort((a, b) => b.count - a.count || a.value.localeCompare(b.value))
    .slice(0, Math.max(0, limit));
}

export function answerValues(core: MockCore, request: SuggestValuesRequest): Response {
  const { kind, prefix, limit } = request;
  const values =
    kind === "tag"
      ? core.data.messages.flatMap((m) => m.tags)
      : kind === "header-name"
        ? core.data.messages.flatMap(() => MOCK_HEADER_NAMES)
        : [];
  return { type: "ValueSuggestions", values: rankValues(values, prefix, limit) };
}
