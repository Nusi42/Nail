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
 * - **One field commits.** Every search field shows the shared typed text;
 *   only the field the user last worked in (`claim`, on focus) commits it,
 *   so two fields of a layout never race with their own scopes.
 * - **Newest wins.** A check that answers after the text moved on is
 *   dropped, so an older query can never replace a newer one; the search
 *   results themselves are keyed by their text (`useSearch`), so a late
 *   answer for an older text is never shown either.
 * - **A folder change searches the new folder** (R2, ADR 0222): picking
 *   another folder while the field holds text runs that text again, in the
 *   picked folder (checked first, like any committed text). While that
 *   search is active, editing the text keeps searching that folder; once
 *   the search ends (an emptied field, Clear), the field searches its own
 *   scope again. The folder pane, not the field, says where to look.
 */
import { useEffect, useEffectEvent, useRef, useState } from "react";
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import { useQueryClient } from "@tanstack/react-query";
import type { AccountId, FolderId, SearchError } from "../../api/generated";
import { LIVE_SEARCH_PAUSE_MS, useDebouncedAction } from "../../state/debounce";
import { checkQuery, useClient } from "../../state/queries";
import { useUi, useUiStoreApi, type SearchState, type UiStore } from "../../state/uiStore";

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

/**
 * Where a committed text is searched: the field's scope, or the folder a
 * folder change narrowed the active search to (`narrowed`, while that
 * search is still the active one).
 */
export function commitScope(
  field: LiveSearchScope,
  active: SearchState | null,
  narrowed: FolderId | null,
): LiveSearchScope {
  const stillNarrowed =
    narrowed !== null && field.folderId === null && active?.folderId === narrowed;
  return stillNarrowed ? { accountId: null, folderId: narrowed } : field;
}

/** A committed text the core found invalid (it was not searched). */
export interface RejectedQuery {
  text: string;
  error: SearchError;
}

const owners = new WeakMap<UiStore, StoreApi<{ owner: string | null }>>();

/** Which search field of a UI store commits the typed text (its id; created on first use). */
export function searchOwnerFor(ui: UiStore): StoreApi<{ owner: string | null }> {
  let store = owners.get(ui);
  if (!store) {
    store = createStore<{ owner: string | null }>()(() => ({ owner: null }));
    owners.set(ui, store);
  }
  return store;
}

/** What the search field needs from live search. */
export interface LiveSearch {
  /** Searches the current text at once (Enter). */
  flush: () => void;
  /** Makes this field the one that commits the typed text (it got the focus). */
  claim: () => void;
  /** The last committed text that was invalid, while it is still typed. */
  rejected: RejectedQuery | null;
}

/**
 * Searches `text` (the field's text) after the typing pause or on `flush`;
 * `scope` is where a new search runs; `field` names the field (only the
 * field that claimed last commits after a pause).
 */
export function useLiveSearch(text: string, scope: LiveSearchScope, field: string): LiveSearch {
  const ownerStore = searchOwnerFor(useUiStoreApi());
  const owner = useStore(ownerStore, (s) => s.owner);
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
  // The folder a folder change narrowed the search to (see `commitScope`).
  const narrowed = useRef<FolderId | null>(null);

  const commit = (value: string, where: LiveSearchScope = scope) => {
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
      const next: SearchState = {
        text: plan.text,
        ...commitScope(where, active, narrowed.current),
        error: null,
      };
      setSearch(next);
    };
    // A failed check (no core answer) leaves the verdict to the search.
    checkQuery(queryClient, client, plan.text).then(run, () => {
      run(null);
    });
  };

  // Before any field had the focus (a test, a search set from elsewhere) nobody commits.
  const flush = useDebouncedAction(text, LIVE_SEARCH_PAUSE_MS, commit, owner === field);

  // A folder change searches the typed text in the new folder.
  const folderId = useUi((s) => s.activeFolderId);
  const shownFolder = useRef(folderId);
  const onFolderChange = useEffectEvent((next: FolderId | null) => {
    if (owner !== field || next === null || latest.current.trim() === "") return;
    narrowed.current = next;
    commit(latest.current, { accountId: null, folderId: next });
  });
  useEffect(() => {
    if (shownFolder.current === folderId) return;
    shownFolder.current = folderId;
    onFolderChange(folderId);
  }, [folderId]);

  const claim = () => {
    if (ownerStore.getState().owner !== field) ownerStore.setState({ owner: field });
  };
  const shown = rejected !== null && rejected.text === text.trim() ? rejected : null;
  return { flush, claim, rejected: shown };
}
