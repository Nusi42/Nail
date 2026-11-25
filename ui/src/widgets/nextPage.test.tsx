import {
  notifyManager,
  QueryClient,
  QueryClientProvider,
  useInfiniteQuery,
} from "@tanstack/react-query";
import { act, render } from "@testing-library/react";
import { afterAll, afterEach, describe, expect, it } from "vitest";
import { must } from "../test/must";
import { useNextPageNearEnd } from "./nextPage";

interface Page {
  offset: number;
  rows: number[];
  total: number;
}

/** One page request the list sent; the test answers it. */
interface Request {
  offset: number;
  answer: () => void;
}

const PAGE = 2;
const TOTAL = 6;

/**
 * A list of `TOTAL` rows in pages of `PAGE`, every request answered by the
 * test; `onRequest` sees each request as it is sent.
 */
function pagedList(onRequest?: (offset: number, queryClient: QueryClient) => void) {
  const requests: Request[] = [];
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  function List() {
    const query = useInfiniteQuery({
      queryKey: ["rows"],
      queryFn: ({ pageParam }) =>
        new Promise<Page>((resolve) => {
          onRequest?.(pageParam, queryClient);
          requests.push({
            offset: pageParam,
            answer: () => {
              const end = Math.min(TOTAL, pageParam + PAGE);
              const rows = Array.from({ length: end - pageParam }, (_, i) => pageParam + i);
              resolve({ offset: pageParam, rows, total: TOTAL });
            },
          });
        }),
      initialPageParam: 0,
      getNextPageParam: (last: Page) => {
        const next = last.offset + last.rows.length;
        return next < last.total ? next : undefined;
      },
    });
    // The list is scrolled to its end: every loaded page asks for the next.
    useNextPageNearEnd(query, true);
    return null;
  }
  render(
    <QueryClientProvider client={queryClient}>
      <List />
    </QueryClientProvider>,
  );
  return { requests, queryClient };
}

// The query cache tells React about changes only when the test flushes them,
// so a change can happen before the list renders the previous one.
let notifications: (() => void)[] = [];
notifyManager.setScheduler((notify) => {
  notifications.push(notify);
});
const flushNotifications = async () => {
  await act(async () => {
    for (;;) {
      await new Promise((resolve) => setTimeout(resolve, 0));
      if (notifications.length === 0) break;
      const pending = notifications;
      notifications = [];
      for (const notify of pending) notify();
    }
  });
};
/** Answers the newest request and lets the list render the result. */
const answerNewest = async (requests: Request[]) => {
  must(requests.at(-1)).answer();
  await flushNotifications();
};

const offsets = (requests: Request[]) => requests.map((r) => r.offset);
/**
 * The refetch of the first page cancelled the request for the second; once
 * the refetch is in, the list asks for the second page again.
 */
const expectAskedAgainAfterRefetch = async (requests: Request[]) => {
  expect(offsets(requests)).toEqual([0, PAGE, 0]);
  await answerNewest(requests);
  expect(offsets(requests)).toEqual([0, PAGE, 0, PAGE]);
};

afterEach(() => {
  notifications = [];
});
afterAll(() => {
  notifyManager.setScheduler((notify) => setTimeout(notify, 0));
});

describe("useNextPageNearEnd", () => {
  it("loads every page while the list stays at its end", async () => {
    const { requests } = pagedList();
    await flushNotifications();
    for (let page = 0; page < TOTAL / PAGE; page++) await answerNewest(requests);
    expect(offsets(requests)).toEqual([0, PAGE, 2 * PAGE]);
  });

  it("asks again when a refetch replaced the next page before the list saw it load", async () => {
    // New mail arrives right after the list asked for the second page, before
    // it rendered "loading the next page": the refetch of the loaded page
    // cancels that request.
    let mailArrived = false;
    const { requests } = pagedList((offset, queryClient) => {
      if (offset === 0 || mailArrived) return;
      mailArrived = true;
      queueMicrotask(() => void queryClient.invalidateQueries({ queryKey: ["rows"] }));
    });
    await flushNotifications();
    await answerNewest(requests);
    await expectAskedAgainAfterRefetch(requests);
    await answerNewest(requests);
    await answerNewest(requests);
    expect(offsets(requests)).toEqual([0, PAGE, 0, PAGE, 2 * PAGE]);
  });

  it("waits for a running refetch instead of cancelling it", async () => {
    const { requests, queryClient } = pagedList();
    await flushNotifications();
    await answerNewest(requests);
    expect(offsets(requests)).toEqual([0, PAGE]);
    // New mail while the list shows "loading the next page": the refetch
    // cancels that request, and the list waits for the refetch rows.
    void queryClient.invalidateQueries({ queryKey: ["rows"] });
    await flushNotifications();
    await expectAskedAgainAfterRefetch(requests);
  });
});
