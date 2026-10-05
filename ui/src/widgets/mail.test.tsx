import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createEnv, renderNode, stubElementSizes, widget } from "../test/harness";
import { filterFolders } from "./FolderPicker";
import { initiallyExpanded, splitQuotes, visibleMessages } from "./ThreadView";
import { applyHint, hintMessageId, hintsFor } from "./SearchBar";
import { threadModeOf, threadStates, threadTemplateData } from "./MessageList";
import {
  addMockConversation,
  MOCK_CONVERSATION_ID,
  MOCK_CONVERSATION_SUBJECT,
} from "../api/mockConversations";
import { must } from "../test/must";

const INBOX = "acc-personal/Inbox";

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
});
afterEach(() => {
  restoreSizes();
});

describe("search-bar", () => {
  it("offers query-language hints that complete the word being typed", async () => {
    const user = userEvent.setup();
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    renderNode(widget("search-bar"), env);
    const input = screen.getByRole("searchbox", { name: "Search mail" });
    await user.click(input);
    const hints = screen.getByRole("list", { name: "Search fields" });
    expect(within(hints).getByText("from:")).toHaveAttribute("data-part", "search-bar.hint.syntax");
    expect(within(hints).getByText("Sender name or address")).toBeInTheDocument();
    await user.type(input, "is");
    expect(within(hints).queryByText("from:")).toBeNull();
    await user.click(within(hints).getByText("is:unread"));
    expect(input).toHaveValue("is:unread ");
    expect(input).toHaveFocus();
  });

  it("keeps the hints while Tab moves into them, hides them when focus leaves (M5b review)", async () => {
    const user = userEvent.setup();
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    renderNode(widget("search-bar"), env);
    const input = screen.getByRole("searchbox", { name: "Search mail" });
    await user.click(input);
    await user.tab();
    const hints = screen.getByRole("list", { name: "Search fields" });
    expect(document.activeElement).not.toBe(document.body);
    expect(hints.contains(document.activeElement)).toBe(true);
    expect(document.activeElement).toHaveAttribute("data-part", "search-bar.hint.button");
    act(() => {
      (document.activeElement as HTMLElement).blur();
    });
    expect(screen.queryByRole("list", { name: "Search fields" })).toBeNull();
  });

  it("shows where a query is wrong while typing", async () => {
    const user = userEvent.setup();
    const env = createEnv({ state: { activeAccountId: "acc-personal" } });
    const { container } = renderNode(widget("search-bar"), env);
    const input = screen.getByRole("searchbox");
    await user.type(input, "from:ada colour:red");
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Unknown search field.");
    expect(alert).toHaveTextContent("At character 10");
    expect(container.querySelector('[data-part="search-bar.error.mark"]')).toHaveTextContent(
      "colour",
    );
    expect(container.querySelector('[data-part="search-bar.error.before"]')).toHaveTextContent(
      "from:ada",
    );
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(input).toHaveAttribute("aria-describedby", alert.id);
    expect(env.transport.requests).toContainEqual({
      type: "CheckQuery",
      text: "from:ada colour:red",
    });
    await user.clear(input);
    await user.type(input, "from:ada");
    await waitFor(() => {
      expect(screen.queryByRole("alert")).toBeNull();
    });
  });

  it("starts a search scoped to the active account and clears it", async () => {
    const user = userEvent.setup();
    const env = createEnv({
      state: { activeAccountId: "acc-work", activeFolderId: "acc-work/Inbox" },
    });
    renderNode(widget("search-bar", { options: { hints: false } }), env);
    const input = screen.getByRole("searchbox");
    await user.type(input, "is:flagged{Enter}");
    expect(env.store.getState().search).toEqual({
      text: "is:flagged",
      accountId: "acc-work",
      folderId: null,
      error: null,
    });
    await user.click(screen.getByRole("button", { name: "Clear search" }));
    expect(env.store.getState().search).toBeNull();
    expect(input).toHaveValue("");
  });

  it("focuses on search.focus requests", () => {
    const env = createEnv();
    renderNode(widget("search-bar"), env);
    act(() => {
      env.store.getState().requestFocus("search");
    });
    expect(screen.getByRole("searchbox")).toHaveFocus();
  });

  it("computes hints", () => {
    expect(hintsFor("")).toContain("before:");
    expect(hintsFor("subject:x is:")).toEqual([
      "is:unread",
      "is:read",
      "is:flagged",
      "is:unflagged",
      "is:answered",
    ]);
    expect(hintsFor("(fr")).toEqual(["from:"]);
    expect(hintsFor("from:")).toEqual([]);
    expect(applyHint("tag:x (fr", "from:")).toBe("tag:x (from:");
    expect(applyHint("has", "has:attachment")).toBe("has:attachment ");
    expect(hintMessageId("size>")).toBe("search-hint-size");
    expect(hintMessageId("is:unread")).toBe("search-hint-is-unread");
  });
});

describe("message-list with search and threads", () => {
  it("shows search hits and reports invalid queries to the search bar", async () => {
    const env = createEnv({
      state: {
        activeAccountId: "acc-personal",
        activeFolderId: INBOX,
        search: { text: "is:flagged", accountId: "acc-work", folderId: null, error: null },
      },
    });
    const { container } = renderNode(widget("message-list"), env);
    const rows = await screen.findAllByRole("option");
    expect(rows.every((r) => r.getAttribute("data-account") === "acc-work")).toBe(true);
    expect(container.querySelector('[data-part="message-list.search-summary"]')).toHaveTextContent(
      /results for “is:flagged”/,
    );
    expect(screen.getByRole("listbox")).toHaveAttribute("data-state", "search");

    act(() => {
      env.store
        .getState()
        .setSearch({ text: "size>", accountId: null, folderId: null, error: null });
    });
    expect(await screen.findByText(/The search could not be run/)).toBeInTheDocument();
    await waitFor(() => {
      expect(env.store.getState().search?.error).toMatchObject({ kind: "unexpected-end" });
    });
  });

  it("shows an empty search result", async () => {
    const env = createEnv({
      state: { search: { text: "from:nobody", accountId: null, folderId: null, error: null } },
    });
    renderNode(widget("message-list"), env);
    expect(await screen.findByText("No messages match this search.")).toBeInTheDocument();
  });

  it("lists conversations with threads=collapsed and selects their newest message", async () => {
    const env = createEnv({ inboxSize: 24, state: { activeFolderId: INBOX } });
    renderNode(
      widget("message-list", { options: { threads: "collapsed" }, rows: ["{subject} {count}"] }),
      env,
    );
    const rows = await screen.findAllByRole("option");
    const thread = must(rows.find((r) => r.dataset.state?.split(" ").includes("thread")));
    expect(thread).toBeDefined();
    expect(env.transport.requests.some((r) => r.type === "ListThreads")).toBe(true);
    fireEvent.click(thread);
    expect(env.store.getState().selectedMessageId).toBe(thread.dataset.message);
    expect(env.store.getState().selectedThreadId).toBe(thread.dataset.thread);
  });

  it("derives thread row data", () => {
    const thread = {
      id: "t",
      subject: "S",
      participants: [
        { name: "A", email: "a@x", me: false, unread: true },
        { name: "B", email: "b@x", me: true, unread: false },
      ],
      message_count: 3,
      unread_count: 1,
      flagged: false,
      has_attachments: true,
      date: "2026-09-29T08:00:00Z",
      preview: "p",
      latest_message_id: "m3",
      latest_message: {
        id: "m3",
        folder_id: "f",
        account_id: "a",
        subject: "Re: S",
        from: { name: "A", email: "a@x" },
        to: [],
        date: "2026-09-29T08:00:00Z",
        flags: { seen: false, flagged: false, answered: false, draft: false },
        has_attachments: false,
        preview: "p",
        thread_id: "t",
        tags: [],
        duplicates: [],
      },
      latest_from: { name: "A", email: "a@x", me: false, unread: true },
      account_ids: ["a", "b"],
    };
    expect(threadStates(thread, true)).toBe("selected unread has-attachments thread");
    // `from` is the newest message's sender (P2), else the last participant.
    expect(threadTemplateData(thread)).toMatchObject({
      from: { name: "A" },
      count: 3,
      unread: true,
      account: "a b",
    });
    expect(threadTemplateData({ ...thread, latest_from: null })).toMatchObject({
      from: { name: "B" },
    });
  });

  it("reads the conversation mode, the pre-P2 `expanded` as one row per message", () => {
    const mode = (threads?: string) =>
      threadModeOf(widget("message-list", threads ? { options: { threads } } : {}));
    expect(mode()).toBe("off");
    expect(mode("off")).toBe("off");
    expect(mode("collapsed")).toBe("collapsed");
    expect(mode("expandable")).toBe("expandable");
    // Before P2 `expanded` meant one row per message (layout reference):
    // a user layout with it keeps its flat list (P2 review).
    expect(mode("expanded")).toBe("off");
  });

  it("shows a conversation's people and its count pill (P2)", async () => {
    const env = createEnv({ inboxSize: 12, state: { activeFolderId: INBOX } });
    addMockConversation(env.transport.data);
    const { container } = renderNode(
      widget("message-list", {
        options: { threads: "collapsed" },
        rows: ["{participants | names(3)}  {count | if-many}", "{subject}"],
      }),
      env,
    );
    await screen.findAllByRole("option");
    const row = must(
      container.querySelector<HTMLElement>(`[data-thread="${MOCK_CONVERSATION_ID}"]`),
    );
    const names = [
      ...row.querySelectorAll<HTMLElement>('[data-part="message-list.row.participant"]'),
    ];
    // Four people: the first two and the newest, the user written "me".
    expect(names.map((n) => n.textContent)).toEqual(["Grace", "me", "Katherine"]);
    expect(names.map((n) => n.dataset.state ?? "")).toEqual(["unread", "me", ""]);
    expect(row.querySelectorAll('[data-part="message-list.row.separator"]')[1]).toHaveTextContent(
      "…",
    );
    expect(row.querySelector('[data-part="message-list.row.count"]')).toHaveTextContent("6");
    expect(row).toHaveTextContent(MOCK_CONVERSATION_SUBJECT);
  });
});

describe("thread-view", () => {
  it("lists the conversation, expands messages in place and opens one", async () => {
    const user = userEvent.setup();
    const env = createEnv({ inboxSize: 24 });
    const thread =
      env.transport.data.messages.find((m) => m.id === `${INBOX}#1`)?.thread_id ?? null;
    act(() => {
      env.store.getState().selectMessage(`${INBOX}#1`);
      env.store.getState().selectThread(thread);
    });
    const { container } = renderNode(widget("thread-view", { options: { expand: "latest" } }), env);
    const view = await screen.findByRole("region", { name: "Conversation" });
    expect(within(view).getByText("3 messages")).toBeInTheDocument();
    const items = container.querySelectorAll('[data-part="thread-view.item"]');
    expect(items).toHaveLength(3);
    // Oldest first; the newest (the selected #1) starts expanded.
    const newest = items[2] as HTMLElement;
    expect(newest.dataset.message).toBe(`${INBOX}#1`);
    expect(newest.dataset.state?.split(" ")).toEqual(
      expect.arrayContaining(["selected", "expanded"]),
    );
    expect(await within(newest).findByText(/Hello Ada/)).toHaveAttribute(
      "data-part",
      "thread-view.body",
    );
    expect(within(newest).getByText("Show quoted text")).toBeInTheDocument();

    const oldest = items[0] as HTMLElement;
    const toggle = must(within(oldest).getAllByRole("button")[0]);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    await user.click(within(oldest).getByRole("button", { name: "Open" }));
    expect(env.store.getState().selectedMessageId).toBe(oldest.dataset.message);
  });

  it("hides itself for single messages and without a thread", async () => {
    const env = createEnv();
    const { container } = renderNode(widget("thread-view"), env);
    expect(container.querySelector('[data-part="thread-view"]')).toBeNull();
    const list = env.transport.data.messages.find((m) => m.id === "acc-work/Archive#list");
    act(() => {
      env.store.getState().selectThread(list?.thread_id ?? null);
    });
    await waitFor(() => {
      expect(
        env.queryClient
          .getQueryCache()
          .findAll({ queryKey: ["thread", list?.thread_id ?? null] })
          .map((q) => q.state.status),
      ).toContain("success");
    });
    expect(container.querySelector('[data-part="thread-view"]')).toBeNull();
  });

  it("shows the newest three messages and folds the earlier ones (P2 quick win)", async () => {
    const user = userEvent.setup();
    const env = createEnv({ inboxSize: 12, state: { activeFolderId: INBOX } });
    const messages = addMockConversation(env.transport.data);
    const newest = must(messages[5]);
    act(() => {
      env.store.getState().selectMessage(newest.id);
      env.store.getState().selectThread(MOCK_CONVERSATION_ID);
    });
    const { container } = renderNode(widget("thread-view", { options: { expand: "none" } }), env);
    const view = await screen.findByRole("region", { name: "Conversation" });
    // The conversation as listed in the Inbox: four there, two from Sent.
    expect(within(view).getByText("6 messages")).toBeInTheDocument();
    expect(env.transport.requests).toContainEqual({
      type: "GetConversation",
      thread_id: MOCK_CONVERSATION_ID,
      folder_id: INBOX,
    });
    const items = () => [
      ...container.querySelectorAll<HTMLElement>('[data-part="thread-view.item"]'),
    ];
    expect(items().map((i) => i.dataset.message)).toEqual(messages.slice(3, 6).map((m) => m.id));
    // The user's own reply from Sent: marked and written "me".
    const mine = must(items()[1]);
    expect(mine.dataset.state?.split(" ")).toContain("from-me");
    expect(within(mine).getByText("me")).toHaveAttribute("data-part", "thread-view.item.from");
    expect(mine.querySelector('[data-part="thread-view.item.avatar"]')).toHaveTextContent("AL");
    const fold = screen.getByRole("button", { name: "3 earlier messages" });
    expect(fold).toHaveAttribute("data-part", "thread-view.fold");
    expect(fold).toHaveAttribute("aria-expanded", "false");
    await user.click(fold);
    expect(items()).toHaveLength(6);
    expect(screen.queryByRole("button", { name: /earlier message/ })).toBeNull();
  });

  it("chooses which messages the strip shows", () => {
    const list = ["a", "b", "c", "d", "e"].map((id) => ({ id }));
    expect(visibleMessages(list, "e", false)).toEqual({ shown: list.slice(2), folded: 2 });
    // The selected message is never folded away.
    expect(visibleMessages(list, "a", false)).toEqual({ shown: list, folded: 0 });
    expect(visibleMessages(list, null, true)).toEqual({ shown: list, folded: 0 });
    expect(visibleMessages(list.slice(0, 3), null, false)).toEqual({
      shown: list.slice(0, 3),
      folded: 0,
    });
  });

  it("splits quoted text and chooses expanded messages", () => {
    expect(splitQuotes("Hi\n> a\n> b\nBye")).toEqual([
      { quoted: false, text: "Hi" },
      { quoted: true, text: "> a\n> b" },
      { quoted: false, text: "Bye" },
    ]);
    const m = { id: "a", flags: { seen: false } } as Parameters<typeof initiallyExpanded>[1];
    expect(initiallyExpanded("unread", m, "z")).toBe(true);
    expect(initiallyExpanded("latest", m, "z")).toBe(false);
    expect(initiallyExpanded("all", m, "z")).toBe(true);
    expect(initiallyExpanded("none", m, "a")).toBe(false);
  });
});

describe("folder-picker", () => {
  it("filters folders and moves the selected message", async () => {
    const user = userEvent.setup();
    const env = createEnv({
      state: {
        activeAccountId: "acc-personal",
        selectedMessageId: `${INBOX}#2`,
        windows: ["move"],
      },
    });
    renderNode(widget("folder-picker"), env);
    await screen.findByRole("button", { name: "Archive" });
    await user.type(screen.getByRole("textbox", { name: "Find a folder" }), "rec");
    expect(screen.queryByRole("button", { name: "Archive" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Receipts" }));
    await waitFor(() => {
      expect(env.store.getState().windows).toEqual([]);
    });
    expect(env.transport.requests).toContainEqual({
      type: "ApplyMailAction",
      targets: [{ kind: "message", message_id: `${INBOX}#2` }],
      action: { type: "move", target: { folder_id: "acc-personal/Inbox/Receipts" } },
    });
  });

  it("moves to the first match on Enter and needs a selection", async () => {
    const user = userEvent.setup();
    const env = createEnv({ state: { activeAccountId: "acc-personal" } });
    renderNode(widget("folder-picker"), env);
    expect(await screen.findByText("Select a message first.")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "Archive" })).toBeDisabled();
    act(() => {
      env.store.getState().selectMessage(`${INBOX}#3`);
    });
    await user.type(screen.getByRole("textbox"), "arch{Enter}");
    await waitFor(() => {
      expect(env.transport.requests).toContainEqual({
        type: "ApplyMailAction",
        targets: [{ kind: "message", message_id: `${INBOX}#3` }],
        action: { type: "move", target: { folder_id: "acc-personal/Archive" } },
      });
    });
  });

  it("filters by name and path", () => {
    const f = (name: string, path: string) =>
      ({ id: path, name, path }) as Parameters<typeof filterFolders>[0][number];
    expect(filterFolders([f("Bills", "Inbox/Bills"), f("Sent", "Sent")], "inbox")).toHaveLength(1);
    expect(filterFolders([f("Bills", "Inbox/Bills")], "  ")).toHaveLength(1);
  });
});

describe("message-view", () => {
  it("marks an unread message as read when it is shown", async () => {
    const env = createEnv({ state: { activeFolderId: INBOX } });
    renderNode(widget("message-list"), env);
    renderNode(widget("message-view"), env);
    await screen.findAllByRole("option");
    act(() => {
      env.store.getState().selectMessage(`${INBOX}#1`);
    });
    await waitFor(() => {
      expect(env.transport.requests).toContainEqual(
        expect.objectContaining({ type: "UpdateFlags", message_ids: [`${INBOX}#1`] }),
      );
    });
  });

  it("leaves messages unread with mark-read=false", async () => {
    const env = createEnv({ state: { activeFolderId: INBOX } });
    renderNode(widget("message-list"), env);
    renderNode(widget("message-view", { options: { "mark-read": false } }), env);
    await screen.findAllByRole("option");
    act(() => {
      env.store.getState().selectMessage(`${INBOX}#1`);
    });
    await screen.findByRole("heading", { name: "Compiler notes for Monday" });
    expect(env.transport.requests.some((r) => r.type === "UpdateFlags")).toBe(false);
  });
});

describe("account-switcher and status-bar", () => {
  it("names the unified inbox and shows connection states", async () => {
    const env = createEnv({ state: { activeAccountId: "all" } });
    const { container } = renderNode(widget("account-switcher"), env);
    const all = await screen.findByRole("button", { name: "All accounts" });
    expect(all).toHaveAttribute("data-kind", "virtual");
    expect(all).toHaveAttribute("aria-pressed", "true");
    const work = await screen.findByRole("button", { name: "Work" });
    await waitFor(() => expect(work).toHaveAttribute("data-status", "online"));
    expect(work).toHaveAttribute("title", "ada@work.example (Online)");
    expect(
      container.querySelector(
        '[data-account="acc-work"] [data-part="account-switcher.item.status"]',
      ),
    ).toHaveAttribute("data-state", "online");
  });

  it("reports accounts that need attention, pending writes and failures", async () => {
    const env = createEnv();
    env.transport.data.statuses[1] = {
      ...must(env.transport.data.statuses[1]),
      state: "auth-error",
      error: "auth_failed",
    };
    renderNode(widget("status-bar", { options: { items: "account-status ops" } }), env);
    const status = screen.getByRole("status", { name: "Status" });
    expect(await within(status).findByText("Work: Sign-in required")).toHaveAttribute(
      "data-state",
      "auth-error",
    );
    act(() => {
      env.store.getState().opQueued(["op-1", "op-2"]);
    });
    expect(within(status).getByText("Saving 2 changes…")).toBeInTheDocument();
    act(() => {
      env.store.getState().opFinished("op-1", "network");
    });
    expect(
      within(status).getByText("A change could not be saved: The server could not be reached."),
    ).toBeInTheDocument();
    fireEvent.click(within(status).getByRole("button", { name: "Dismiss" }));
    expect(within(status).getByText("Saving one change…")).toBeInTheDocument();
  });
});
