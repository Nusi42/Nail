/**
 * The message list's filter chips (M5b WP162; `nail_api::views::ListFilter`):
 * each chip adds a term of the existing query language, so filtering is a
 * search scoped to the folder the list shows (or added to the search the
 * user typed). `ListFilter::search_term` on the Rust side; same table.
 */
import type { AccountId, FolderId, ListFilter } from "../api/generated";
import type { SearchScope } from "../state/queries";

/** `ListFilter::ALL`. */
export const LIST_FILTERS: readonly ListFilter[] = ["all", "unread", "flagged", "attachments"];

/** `ListFilter::search_term`. */
export const LIST_FILTER_TERMS: Readonly<Record<ListFilter, string | null>> = {
  all: null,
  unread: "is:unread",
  flagged: "is:flagged",
  attachments: "has:attachment",
};

/** Whether a JSON value names a filter (`list.filter "<filter>"`). */
export function isListFilter(value: unknown): value is ListFilter {
  return typeof value === "string" && (LIST_FILTERS as readonly string[]).includes(value);
}

/**
 * The search the list runs for a filter: the user's search (in parentheses,
 * so its `OR`s stay inside) plus the term,
 * else the term within the shown folder; `null` for "All" (or when no
 * folder is shown and nothing is searched).
 */
export function filteredScope(
  filter: ListFilter,
  search: SearchScope | null,
  accountId: AccountId | null,
  folderId: FolderId | null,
): SearchScope | null {
  const term = LIST_FILTER_TERMS[filter];
  if (term === null) return null;
  if (search !== null) {
    return {
      text: `(${search.text}) ${term}`,
      accountId: search.accountId,
      folderId: search.folderId,
    };
  }
  if (folderId === null) return null;
  return { text: term, accountId, folderId };
}
