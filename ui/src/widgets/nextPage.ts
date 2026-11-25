import { useEffect } from "react";

/** The parts of an infinite query that decide when its next page loads. */
export interface PagedQuery {
  hasNextPage: boolean;
  isFetching: boolean;
  dataUpdatedAt: number;
  fetchNextPage: () => Promise<unknown>;
}

/**
 * Loads the next page of `query` while `nearEnd` holds (the list shows rows
 * close to the last loaded one).
 *
 * It waits while any fetch of the list runs and asks again after each one:
 * a refetch (new mail, a resync) cancels a next-page request that started
 * just before it, and when that happens before the list rendered the
 * request, "loading the next page" never changed for React and the list
 * stopped at the loaded pages for good. Waiting also keeps a next-page
 * request from cancelling the refetch, whose new rows would be lost.
 */
export function useNextPageNearEnd(query: PagedQuery, nearEnd: boolean): void {
  const { hasNextPage, isFetching, dataUpdatedAt, fetchNextPage } = query;
  useEffect(() => {
    if (hasNextPage && !isFetching && nearEnd) void fetchNextPage();
    // `dataUpdatedAt`: every finished fetch asks again, also one whose start
    // the list never rendered.
  }, [hasNextPage, isFetching, dataUpdatedAt, fetchNextPage, nearEnd]);
}
