import { QueryClient, type InfiniteData } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import type { SearchHit } from "../api/generated";
import { createMockData } from "../api/mockData";
import { must } from "../test/must";
import { patchSummaries } from "./mailOps";
import { nextSearchOffset, queryKeys, type SearchPage } from "./queries";

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

describe("patchSummaries on search results", () => {
  /** Removes the first hit of a cached search page and returns the page after it. */
  function removeFirst(page: SearchPage): SearchPage {
    const queryClient = new QueryClient();
    const key = queryKeys.search("is:flagged", null, null, 100);
    queryClient.setQueryData<InfiniteData<SearchPage>>(key, { pages: [page], pageParams: [0] });
    patchSummaries(queryClient, [must(page.hits[0]).message.id], () => null);
    return must(queryClient.getQueryData<InfiniteData<SearchPage>>(key)?.pages[0]);
  }

  it("takes a removed hit out of the total, so a complete search asks for no next page", () => {
    // A moved message leaves the list at once; the core still lists it until
    // the move is written, so a next page then would bring it back.
    const after = removeFirst({ offset: 0, total: 3, hits: hits(3), error: null });
    expect(after.hits).toHaveLength(2);
    expect(after.total).toBe(2);
    expect(nextSearchOffset(after)).toBeUndefined();
    expect(
      nextSearchOffset(removeFirst({ offset: 0, total: 5, hits: hits(2, 3), error: null })),
    ).toBeUndefined();
  });

  it("keeps the next page of a search with more hits", () => {
    expect(
      nextSearchOffset(removeFirst({ offset: 0, total: 10, hits: hits(4), error: null })),
    ).toBe(3);
  });
});
