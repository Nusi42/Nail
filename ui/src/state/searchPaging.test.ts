import { describe, expect, it } from "vitest";
import type { SearchHit } from "../api/generated";
import { createMockData } from "../api/mockData";
import { nextSearchOffset } from "./queries";

const summaries = createMockData().messages;

function hits(local: number, server = 0): SearchHit[] {
  return [
    ...summaries.slice(0, local).map((message) => ({ message, score: 1 })),
    ...summaries
      .slice(local, local + server)
      .map((message) => ({ message, score: 0, from_server: true })),
  ];
}

describe("nextSearchOffset", () => {
  it("pages over local hits", () => {
    expect(nextSearchOffset({ offset: 0, total: 10, hits: hits(4), error: null })).toBe(4);
    expect(nextSearchOffset({ offset: 8, total: 10, hits: hits(2), error: null })).toBeUndefined();
  });

  it("does not skip local hits when server hits come on top of the first page", () => {
    // 10 local matches, 3 more found only by the server (in `total`).
    const first = { offset: 0, total: 13, hits: hits(4, 3), error: null };
    expect(nextSearchOffset(first)).toBe(4);
    expect(nextSearchOffset({ offset: 8, total: 10, hits: hits(2), error: null })).toBeUndefined();
    // All local hits on the first page: no further page.
    expect(
      nextSearchOffset({ offset: 0, total: 7, hits: hits(4, 3), error: null }),
    ).toBeUndefined();
  });

  it("stops on errors and empty pages", () => {
    expect(
      nextSearchOffset({ offset: 0, total: 3, hits: hits(0, 3), error: null }),
    ).toBeUndefined();
    expect(
      nextSearchOffset({
        offset: 0,
        total: 10,
        hits: hits(4),
        error: { message: "x", kind: "unexpected-end", start: 0, end: 1 },
      }),
    ).toBeUndefined();
  });
});
