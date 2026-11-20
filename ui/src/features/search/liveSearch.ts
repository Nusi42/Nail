/**
 * Live search (R1 WR2, ADR 0219): the search field searches what is typed
 * once typing paused for `LIVE_SEARCH_PAUSE_MS`, and at once on Enter.
 *
 * - **Committed searches.** A search is committed when the pause settles or
 *   Enter is pressed; both go through {@link planLiveSearch}. Nothing is
 *   sent while the text is unchanged (the same trimmed text as the active
 *   search), and an emptied field ends the search.
 * - **Checked first.** A committed text is checked (`CheckQuery`, cached per
 *   text) before it is searched. An invalid one is not searched: the list
 *   keeps the last valid results and the field shows the mistake.
 * - **Newest wins.** A check that answers after the text moved on is
 *   dropped, so an older query can never replace a newer one; the search
 *   results themselves are keyed by their text (`useSearch`), so a late
 *   answer for an older text is never shown either.
 */
import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import type { AccountId, FolderId, SearchError } from "../../api/generated";
import { LIVE_SEARCH_PAUSE_MS, useDebouncedAction } from "../../state/debounce";
import { checkQuery, useClient } from "../../state/queries";
import { useUi, type SearchState } from "../../state/uiStore";

/** Where a search from the field runs. */
export interface LiveSearchScope {
  accountId: AccountId | null;
  folderId: FolderId | null;
}

/** What committing `text` does to the active search. */
export type LiveSearchPlan =
  { kind: "none" } | { kind: "clear" } | { kind: "search"; text: string };

/**
 * The step for a committed text: nothing while it equals the active search
 * (or both are empty), clear for an empty field, else search the trimmed text.
 */
export function planLiveSearch(text: string, active: SearchState | null): LiveSearchPlan {
  const query = text.trim();
  if (query === "") return active === null ? { kind: "none" } : { kind: "clear" };
  if (active?.text === query) return { kind: "none" };
  return { kind: "search", text: query };
}

/** A committed text the core found invalid (it was not searched). */
export interface RejectedQuery {
  text: string;
  error: SearchError;
}

/** What the search field needs from live search. */
export interface LiveSearch {
  /** Searches the current text at once (Enter). */
  flush: () => void;
  /** The last committed text that was invalid, while it is still typed. */
  rejected: RejectedQuery | null;
}

/**
 * Searches `text` (the field's text) after the typing pause or on `flush`;
 * `scope` is where a new search runs.
 */
export function useLiveSearch(text: string, scope: LiveSearchScope): LiveSearch {
  const client = useClient();
  const queryClient = useQueryClient();
  const active = useUi((s) => s.search);
  const setSearch = useUi((s) => s.setSearch);
  const [rejected, setRejected] = useState<RejectedQuery | null>(null);
  // The text as typed now: a check that answers for another text is stale.
  const latest = useRef(text);
  useEffect(() => {
    latest.current = text;
  }, [text]);

  const commit = (value: string) => {
    const plan = planLiveSearch(value, active);
    if (plan.kind === "none") return;
    if (plan.kind === "clear") {
      if (rejected !== null) setRejected(null);
      setSearch(null);
      return;
    }
    const current = () => latest.current.trim() === plan.text;
    const run = (error: SearchError | null) => {
      if (!current()) return;
      if (error) {
        setRejected({ text: plan.text, error });
        return;
      }
      if (rejected !== null) setRejected(null);
      const next: SearchState = { text: plan.text, ...scope, error: null };
      setSearch(next);
    };
    // A failed check (no core answer) leaves the verdict to the search.
    checkQuery(queryClient, client, plan.text).then(run, () => {
      run(null);
    });
  };

  const flush = useDebouncedAction(text, LIVE_SEARCH_PAUSE_MS, commit);
  const shown = rejected !== null && rejected.text === text.trim() ? rejected : null;
  return { flush, rejected: shown };
}
