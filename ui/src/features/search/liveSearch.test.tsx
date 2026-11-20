import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Request, Response } from "../../api/generated";
import { createEnv, renderNode, widget, type Env } from "../../test/harness";
import { requestsOf } from "../../test/env";
import { must } from "../../test/must";
import { INBOX } from "../../test/mockIds";
import { LIVE_SEARCH_PAUSE_MS } from "../../state/debounce";
import { keepsSearchResults, queryKeys } from "../../state/queries";
import type { SearchState } from "../../state/uiStore";
import { planLiveSearch } from "./liveSearch";

const active = (text: string): SearchState => ({
  text,
  accountId: null,
  folderId: null,
  error: null,
});

/** The search bar of the personal Inbox (no hints, so nothing else overlays). */
function searchBar(
  env: Env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } }),
) {
  const view = renderNode(widget("search-bar", { options: { hints: false } }), env);
  const input = screen.getByRole("searchbox", { name: "Search mail" });
  return { env, input, view, user: userEvent.setup() };
}

/** Every value the store's search took, from now on. */
function searchHistory(env: Env): (SearchState | null)[] {
  const seen: (SearchState | null)[] = [];
  env.store.subscribe((state, previous) => {
    if (state.search !== previous.search) seen.push(state.search);
  });
  return seen;
}

/** Waits (inside `act`) for `ms`. */
async function pause(ms: number): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, ms));
  });
}

/** Types `keys` and waits until the store searches `text`. */
async function typeAndSearch(
  setup: ReturnType<typeof searchBar>,
  keys: string,
  text: string,
): Promise<void> {
  await setup.user.type(setup.input, keys);
  await waitFor(() => {
    expect(setup.env.store.getState().search?.text).toBe(text);
  });
}

/** Holds the mock core's answer to `CheckQuery` for `text` until released. */
function holdCheck(env: Env, text: string): () => void {
  let release: (() => void) | null = null;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const send = env.transport.send.bind(env.transport);
  env.transport.send = async (request: Request): Promise<Response> => {
    if (request.type === "CheckQuery" && request.text === text) await gate;
    return send(request);
  };
  return () => {
    act(() => {
      must(release)();
    });
  };
}

describe("planLiveSearch", () => {
  it("searches the trimmed text, clears an emptied field and skips unchanged text", () => {
    expect(planLiveSearch("  from:ada ", null)).toEqual({ kind: "search", text: "from:ada" });
    expect(planLiveSearch("from:ada ", active("from:ada"))).toEqual({ kind: "none" });
    expect(planLiveSearch("from:ad", active("from:ada"))).toEqual({
      kind: "search",
      text: "from:ad",
    });
    expect(planLiveSearch("  ", active("from:ada"))).toEqual({ kind: "clear" });
    expect(planLiveSearch("", null)).toEqual({ kind: "none" });
  });
});

describe("keepsSearchResults", () => {
  it("keeps the shown results only for a new text in the same scope", () => {
    const key = queryKeys.search("a", "acc", null, 50);
    expect(keepsSearchResults(key, queryKeys.search("ab", "acc", null, 50))).toBe(true);
    expect(keepsSearchResults(key, queryKeys.search("ab", "other", null, 50))).toBe(false);
    expect(keepsSearchResults(key, queryKeys.search("ab", "acc", "acc/Inbox", 50))).toBe(false);
    expect(keepsSearchResults(undefined, key)).toBe(false);
  });
});

describe("live search", () => {
  it("searches once typing pauses, without Enter", async () => {
    const { env, input, user } = searchBar();
    const seen = searchHistory(env);
    await user.type(input, "is:flagged");
    // Nothing is searched while typing.
    expect(env.store.getState().search).toBeNull();
    await waitFor(
      () => {
        expect(env.store.getState().search).toEqual({
          text: "is:flagged",
          accountId: "acc-personal",
          folderId: null,
          error: null,
        });
      },
      { timeout: LIVE_SEARCH_PAUSE_MS * 4 },
    );
    // One committed search for the settled text, not one per letter.
    expect(seen).toHaveLength(1);
    // Clearing the field by typing ends the search after the pause.
    await user.clear(input);
    await waitFor(
      () => {
        expect(env.store.getState().search).toBeNull();
      },
      { timeout: LIVE_SEARCH_PAUSE_MS * 4 },
    );
  });

  it("searches at once on Enter", async () => {
    const { env, input, user } = searchBar();
    await user.type(input, "from:ada{Enter}");
    await waitFor(
      () => {
        expect(env.store.getState().search?.text).toBe("from:ada");
      },
      // Well before the typing pause would have run it.
      { timeout: LIVE_SEARCH_PAUSE_MS / 2 },
    );
  });

  it("sends nothing while the text is unchanged", async () => {
    const setup = searchBar();
    await typeAndSearch(setup, "from:ada{Enter}", "from:ada");
    const seen = searchHistory(setup.env);
    const checks = requestsOf(setup.env, "CheckQuery").length;
    // A trailing space and Enter again: the same query.
    await setup.user.type(setup.input, " {Enter}");
    await pause(LIVE_SEARCH_PAUSE_MS * 2);
    expect(seen).toEqual([]);
    // Only the live syntax check of the typed text may ask the core.
    expect(
      requestsOf(setup.env, "CheckQuery")
        .slice(checks)
        .every((r) => r.text === "from:ada "),
    ).toBe(true);
  });

  it("drops a check that answers after the text moved on", async () => {
    const setup = searchBar();
    const release = holdCheck(setup.env, "from:ada");
    const seen = searchHistory(setup.env);
    await setup.user.type(setup.input, "from:ada{Enter}");
    await typeAndSearch(setup, " is:flagged{Enter}", "from:ada is:flagged");
    // The older query's answer arrives last and changes nothing.
    release();
    await pause(20);
    expect(setup.env.store.getState().search?.text).toBe("from:ada is:flagged");
    expect(seen.map((s) => s?.text)).toEqual(["from:ada is:flagged"]);
  });

  it("keeps the last valid search while the text is invalid and shows the mistake", async () => {
    const setup = searchBar();
    const { env, input, user, view } = setup;
    await typeAndSearch(setup, "is:flagged{Enter}", "is:flagged");
    await user.type(input, " colour:red{Enter}");
    await waitFor(() => {
      expect(input).toHaveAttribute("aria-invalid", "true");
    });
    await pause(LIVE_SEARCH_PAUSE_MS * 2);
    expect(env.store.getState().search?.text).toBe("is:flagged");

    const field = must(view.container.querySelector('[data-part="search-bar.field"]'));
    expect(field).toHaveAttribute("data-state", "invalid");
    expect(field.querySelector('[data-part="problem"]')).toHaveAttribute(
      "data-kind",
      "unknown-field",
    );
    const tip = must(document.getElementById(must(input.getAttribute("aria-describedby"))));
    expect(tip).toHaveAttribute("data-part", "problem.details");
    expect(tip).toHaveAttribute("data-mode", "follow");
    expect(tip).toHaveAttribute("role", "tooltip");
    expect(tip.querySelector('[data-part="search-bar.error"]')).not.toBeNull();
    expect(tip).toHaveTextContent("Unknown search field.");
    expect(tip).toHaveTextContent("The list still shows the results for “is:flagged”.");
    expect(tip.querySelector('[data-part="search-bar.error.mark"]')).toHaveTextContent("colour");
  });

  it("shows the explanation only on hover or focus", async () => {
    const { input, user, view } = searchBar();
    await user.type(input, "frm:ada");
    await waitFor(() => {
      expect(input).toHaveAttribute("aria-invalid", "true");
    });
    const tip = must(document.getElementById(must(input.getAttribute("aria-describedby"))));
    expect(tip).toBeVisible();
    expect(tip).toHaveTextContent("Fix the query to search.");
    const form = must(view.container.querySelector('[data-part="search-bar"]'));
    // Focus alone keeps it open (the pointer clicked in, then left).
    fireEvent.mouseLeave(form);
    expect(tip).toBeVisible();
    act(() => {
      input.blur();
    });
    expect(tip).not.toBeVisible();
    fireEvent.mouseEnter(form);
    expect(tip).toBeVisible();
    fireEvent.mouseLeave(form);
    expect(tip).not.toBeVisible();
    // The described-by text stays in the DOM for screen readers.
    expect(input).toHaveAttribute("aria-describedby", tip.id);
  });

  it("announces a new mistake while the field keeps the focus", async () => {
    const { input, user, view } = searchBar();
    await user.type(input, "frm:ada");
    const form = must(view.container.querySelector('[data-part="search-bar"]'));
    const status = must(form.querySelector('[data-part="search-bar.status"]'));
    expect(status).toHaveAttribute("role", "status");
    await waitFor(() => {
      expect(status).toHaveTextContent("Unknown search field. (At character 1)");
    });
    await user.clear(input);
    await user.type(input, "is:flagged");
    await waitFor(() => {
      expect(status).toHaveTextContent(/^$/);
    });
  });

  it("commits only from the search field the user works in", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    const seen = searchHistory(env);
    // Two fields of one layout: one searches the open folder, one everywhere.
    renderNode(widget("search-bar", { options: { hints: false, scope: "folder" } }), env);
    renderNode(widget("search-bar", { options: { hints: false, scope: "all" } }), env);
    const everywhere = must(screen.getAllByRole("searchbox", { name: "Search mail" })[1]);
    await userEvent.setup().type(everywhere, "from:ada");
    await waitFor(() => {
      expect(env.store.getState().search?.text).toBe("from:ada");
    });
    await pause(LIVE_SEARCH_PAUSE_MS * 2);
    // One search, where the typing field searches: no second one from the other field.
    expect(seen).toEqual([{ text: "from:ada", accountId: null, folderId: null, error: null }]);
  });
});
