/**
 * R2 (ADR 0222): a folder change while the field holds text searches that
 * text in the new folder; editing it keeps searching there until the
 * search ends.
 */
import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createEnv, renderNode, widget } from "../../test/harness";
import { requestsOf } from "../../test/env";
import { INBOX, SENT } from "../../test/mockIds";
import type { SearchState } from "../../state/uiStore";
import { commitScope } from "./liveSearch";

const ACCOUNT = { accountId: "acc-personal", folderId: null };

function searchBar() {
  const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
  renderNode(widget("search-bar", { options: { hints: false } }), env);
  const input = screen.getByRole("searchbox", { name: "Search mail" });
  return { env, input, user: userEvent.setup() };
}

const searched = (text: string, folderId: string | null, accountId: string | null = null) => ({
  text,
  accountId,
  folderId,
  error: null,
});

describe("commitScope", () => {
  it("keeps a narrowed folder only while its search is the active one", () => {
    const narrowed: SearchState = searched("x", SENT);
    expect(commitScope(ACCOUNT, narrowed, SENT)).toEqual({ accountId: null, folderId: SENT });
    expect(commitScope(ACCOUNT, null, SENT)).toEqual(ACCOUNT);
    expect(commitScope(ACCOUNT, searched("x", null, "acc-personal"), SENT)).toEqual(ACCOUNT);
    expect(commitScope(ACCOUNT, narrowed, null)).toEqual(ACCOUNT);
    // A field of its own folder searches that folder anyway.
    const folderField = { accountId: "acc-personal", folderId: INBOX };
    expect(commitScope(folderField, narrowed, SENT)).toEqual(folderField);
  });
});

describe("a folder change with a typed search", () => {
  it("searches the text in the new folder and keeps that folder while editing", async () => {
    const { env, input, user } = searchBar();
    await user.type(input, "is:flagged{Enter}");
    await waitFor(() => {
      expect(env.store.getState().search).toEqual(searched("is:flagged", null, "acc-personal"));
    });

    act(() => {
      env.store.getState().selectFolder(SENT);
    });
    await waitFor(() => {
      expect(env.store.getState().search).toEqual(searched("is:flagged", SENT));
    });
    expect(input).toHaveValue("is:flagged");

    // Editing the text keeps searching the picked folder.
    await user.type(input, " is:unread{Enter}");
    await waitFor(() => {
      expect(env.store.getState().search).toEqual(searched("is:flagged is:unread", SENT));
    });

    // Once the search ends, the field searches its own scope again.
    await user.clear(input);
    await user.type(input, "{Enter}");
    expect(env.store.getState().search).toBeNull();
    await user.type(input, "from:ada{Enter}");
    await waitFor(() => {
      expect(env.store.getState().search).toEqual(searched("from:ada", null, "acc-personal"));
    });
  });

  it("does not search an invalid text, nor an empty field", async () => {
    const { env, input, user } = searchBar();
    await user.type(input, "colour:red{Enter}");
    await waitFor(() => {
      expect(requestsOf(env, "CheckQuery")).toContainEqual({
        type: "CheckQuery",
        text: "colour:red",
      });
    });
    act(() => {
      env.store.getState().selectFolder(SENT);
    });
    await waitFor(() => {
      expect(requestsOf(env, "CheckQuery").length).toBeGreaterThanOrEqual(1);
    });
    expect(env.store.getState().search).toBeNull();
    expect(requestsOf(env, "Search")).toEqual([]);

    await user.clear(input);
    act(() => {
      env.store.getState().selectFolder(INBOX);
    });
    expect(env.store.getState().search).toBeNull();
  });

  it("leaves a search another part set for the new folder alone", async () => {
    const { env, input, user } = searchBar();
    await user.click(input);
    act(() => {
      const state = env.store.getState();
      state.selectFolder(SENT);
      state.setSearchInput("tag:x");
      state.setSearch(searched("tag:x", SENT, "acc-personal"));
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(env.store.getState().search).toEqual(searched("tag:x", SENT, "acc-personal"));
    expect(requestsOf(env, "CheckQuery")).toEqual([]);
  });
});
