/**
 * M1a flows through the whole app (default layout, key bindings, windows,
 * events) against the mock core.
 */
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import { createEnv, setWindowWidth, stubElementSizes, type Env } from "./test/harness";

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
  setWindowWidth(1200);
});
afterEach(() => {
  restoreSizes();
  setWindowWidth(1024);
});

function renderApp(env: Env = createEnv({ state: { connection: "connecting" } })) {
  const result = render(
    <App client={env.client} queryClient={env.queryClient} store={env.store} />,
  );
  return { ...result, env };
}

/**
 * Waits are bounded by event-loop turns, not by wall-clock time: the mock
 * core answers within a turn or two and nothing here needs a real delay, so
 * a loaded machine (cargo builds in parallel) makes the waits slower but
 * never flaky, unlike `findBy*` / `waitFor` with their 1 s budget.
 */
const TURNS = 100;

/** One event-loop turn inside `act`, so React commits what it received. */
async function turn(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

/** Turns until `query` finds something. */
async function settleUntil<T>(query: () => T | null, what: string): Promise<T> {
  for (let n = 0; n < TURNS; n++) {
    const found = query();
    if (found !== null) return found;
    await turn();
  }
  throw new Error(`${what} did not appear within ${TURNS} event-loop turns`);
}

/**
 * Turns until `assertion` passes (a turn-bounded `waitFor`). It starts with
 * a turn, so what the last event started (an action's request and the store
 * updates after its answer) finishes inside `act`.
 */
async function eventually(assertion: () => void): Promise<void> {
  let failure: unknown = null;
  for (let n = 0; n < TURNS; n++) {
    await turn();
    try {
      assertion();
      return;
    } catch (error) {
      failure = error;
    }
  }
  throw failure;
}

/** The message list's rows once the app has loaded. */
async function ready() {
  return settleUntil(() => {
    const rows = screen.queryAllByRole("option");
    return rows.length > 0 ? rows : null;
  }, "message rows");
}

describe("M1a flows", () => {
  it("shows both accounts in the unified inbox", async () => {
    const { env } = renderApp();
    const rows = await ready();
    const accounts = new Set(rows.map((r) => r.getAttribute("data-account")));
    expect(accounts).toEqual(new Set(["acc-personal", "acc-work"]));
    expect(env.store.getState().activeFolderId).toBe("all/inbox");
    const tree = screen.getByRole("tree", { name: "Folders" });
    expect(within(tree).getByRole("treeitem", { name: /Personal/ })).toHaveAttribute(
      "data-depth",
      "0",
    );
  });

  it("writes a message with c, sends it with ctrl+enter and undoes the send", async () => {
    const user = userEvent.setup();
    const { env } = renderApp();
    await ready();
    fireEvent.keyDown(document.body, { key: "c" });
    const dialog = await settleUntil(
      () => screen.queryByRole("dialog", { name: "New message" }),
      "'dialog', { name: 'New message' }",
    );
    expect(dialog).toHaveAttribute("data-window", "composer");
    await eventually(() =>
      expect(within(dialog).getByRole("combobox", { name: "From" })).toHaveFocus(),
    );
    // Global mail keys do not fire inside the dialog.
    fireEvent.keyDown(within(dialog).getByRole("button", { name: "Send" }), { key: "r" });
    expect(env.store.getState().composer?.context).toBeNull();

    await user.type(within(dialog).getByRole("textbox", { name: "To" }), "grace@navy.example");
    await user.type(within(dialog).getByRole("textbox", { name: "Subject" }), "Lunch");
    const body = within(dialog).getByRole("textbox", { name: "Message" });
    await user.click(body);
    fireEvent.keyDown(body, { key: "Enter", ctrlKey: true });
    await eventually(() => expect(screen.queryByRole("dialog")).toBeNull());

    const status = screen.getByRole("status", { name: "Status" });
    // The countdown follows the wall clock, so any second of the delay is fine.
    expect(
      within(status).getByText(/^Sending in (one second|([2-9]|10) seconds)$/),
    ).toBeInTheDocument();
    await user.click(within(status).getByRole("button", { name: "Undo" }));
    const reopened = await settleUntil(
      () => screen.queryByRole("dialog", { name: "New message" }),
      "'dialog', { name: 'New message' }",
    );
    expect(within(reopened).getByRole("textbox", { name: "Subject" })).toHaveValue("Lunch");
    expect(env.transport.data.outbox).toEqual([]);

    // Escape closes the window; the message stays for later.
    fireEvent.keyDown(within(reopened).getByRole("textbox", { name: "Subject" }), {
      key: "Escape",
    });
    await eventually(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(env.store.getState().composer?.subject).toBe("Lunch");
  });

  it("reports the send result from SendStatus events", async () => {
    const { env } = renderApp();
    await ready();
    const entry = {
      id: "out-9",
      draft_id: "d",
      account_id: "acc-personal",
      identity_id: "ada@example.org",
      subject: "S",
      recipients: [],
      send_at: new Date().toISOString(),
      state: "sending" as const,
      attempts: 0,
      error: null,
      message: null,
    };
    act(() => env.store.getState().setLastSend(entry));
    const status = screen.getByRole("status", { name: "Status" });
    expect(within(status).getByText("Sending…")).toBeInTheDocument();
    act(() =>
      env.transport.emit({
        type: "SendStatus",
        outbox_id: "out-9",
        state: "failed",
        error: "network",
        message: "x",
      }),
    );
    expect(
      within(status).getByText("Sending failed: The server could not be reached."),
    ).toBeInTheDocument();
  });

  it("shows SendProgress events of a large upload until the send finishes", async () => {
    const { env } = renderApp();
    await ready();
    const entry = {
      id: "out-7",
      draft_id: "d",
      account_id: "acc-personal",
      identity_id: "ada@example.org",
      subject: "S",
      recipients: [],
      send_at: new Date().toISOString(),
      state: "sending" as const,
      attempts: 0,
      error: null,
      message: null,
    };
    act(() => env.store.getState().setLastSend(entry));
    const status = screen.getByRole("status", { name: "Status" });
    act(() =>
      env.transport.emit({
        type: "SendProgress",
        outbox_id: "out-7",
        sent_bytes: 3_276_800,
        total_bytes: 13_107_200,
      }),
    );
    expect(within(status).getByText("Sending… 25% uploaded")).toBeInTheDocument();
    act(() =>
      env.transport.emit({
        type: "SendStatus",
        outbox_id: "out-7",
        state: "sent",
        error: null,
        message: null,
      }),
    );
    expect(env.store.getState().sendProgress).toEqual({});
    expect(within(status).getByText("Message sent.")).toBeInTheDocument();
  });

  it("archives with e, moves with v and marks flags with s", async () => {
    const user = userEvent.setup();
    const { env } = renderApp();
    const rows = await ready();
    const first = rows[0] as HTMLElement;
    const firstId = first.dataset.message ?? "";
    fireEvent.click(first);
    fireEvent.keyDown(first, { key: "s" });
    await eventually(() =>
      expect(env.transport.requests).toContainEqual(
        expect.objectContaining({ type: "UpdateFlags", message_ids: [firstId] }),
      ),
    );
    fireEvent.keyDown(document.body, { key: "e" });
    await eventually(() =>
      expect(env.transport.requests).toContainEqual({
        type: "MoveMessages",
        message_ids: [firstId],
        target: { role: "archive" },
      }),
    );
    const next = env.store.getState().selectedMessageId;
    expect(next).not.toBe(firstId);
    expect(next).not.toBeNull();

    fireEvent.keyDown(document.body, { key: "v" });
    const picker = await settleUntil(
      () => screen.queryByRole("dialog", { name: "Move to folder" }),
      "'dialog', { name: 'Move to folder' }",
    );
    await user.click(
      await settleUntil(
        () => within(picker).queryByRole("button", { name: "Trash" }),
        "'button', { name: 'Trash' }",
      ),
    );
    await eventually(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(env.transport.requests).toContainEqual({
      type: "MoveMessages",
      message_ids: [next],
      target: { folder_id: "all/trash" },
    });
    // The mock core replays the writes with OpStatus events; the status bar
    // follows them until the queue is empty.
    await eventually(() => expect(env.store.getState().pendingOps).toEqual([]));
  });

  it("searches with / and the query language, and clears with Escape", async () => {
    const user = userEvent.setup();
    const { env } = renderApp();
    await ready();
    fireEvent.keyDown(document.body, { key: "/" });
    const input = screen.getByRole("searchbox", { name: "Search mail" });
    expect(input).toHaveFocus();
    await user.type(input, "from:grace{Enter}");
    await eventually(() =>
      expect(screen.getByRole("listbox", { name: "Messages" })).toHaveAttribute(
        "data-state",
        "search",
      ),
    );
    const hits = await ready();
    expect(hits.length).toBeGreaterThan(0);
    expect(env.store.getState().search?.accountId).toBe("all");
    fireEvent.keyDown(input, { key: "Escape" });
    expect(env.store.getState().search).toBeNull();
    expect(input).toHaveValue("");
  });

  it("opens settings and account setup from the main toolbar", async () => {
    const user = userEvent.setup();
    renderApp();
    await ready();
    const toolbar = screen.getByRole("toolbar", { name: "Main toolbar" });
    await user.click(within(toolbar).getByRole("button", { name: "Settings" }));
    const settings = await settleUntil(
      () => screen.queryByRole("dialog", { name: "Settings" }),
      "'dialog', { name: 'Settings' }",
    );
    expect(within(settings).getByRole("tab", { name: "Accounts" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await user.click(within(settings).getByRole("tab", { name: "Virtual accounts" }));
    expect(
      await settleUntil(
        () => within(settings).queryByText(/No virtual accounts yet/),
        "/No virtual accounts yet/",
      ),
    ).toBeVisible();
    await user.click(within(toolbar).getByRole("button", { name: "Add account" }));
    const setup = await settleUntil(
      () => screen.queryByRole("dialog", { name: "Add account" }),
      "'dialog', { name: 'Add account' }",
    );
    expect(setup).toHaveAttribute("data-state", "active");
    expect(settings).not.toHaveAttribute("data-state");
    await user.click(within(setup).getByRole("button", { name: "Close" }));
    await eventually(() => expect(settings).toHaveAttribute("data-state", "active"));
  });

  it("applies account status and account changes pushed by the core", async () => {
    const { env } = renderApp();
    await ready();
    const work = await settleUntil(
      () => screen.queryByRole("button", { name: "Work" }),
      "'button', { name: 'Work' }",
    );
    await eventually(() => expect(work).toHaveAttribute("data-status", "online"));
    act(() =>
      env.transport.emit({
        type: "AccountStatus",
        status: {
          account_id: "acc-work",
          state: "offline",
          since: new Date().toISOString(),
          error: "network",
          message: "down",
        },
      }),
    );
    // Query observers are notified asynchronously.
    await eventually(() => expect(work).toHaveAttribute("data-status", "offline"));
    expect(screen.getByRole("status", { name: "Status" })).toHaveTextContent("Work: Offline");

    env.transport.data.accounts[1] = {
      ...(env.transport.data.accounts[1] as (typeof env.transport.data.accounts)[number]),
      name: "Office",
    };
    act(() => env.transport.emit({ type: "AccountsChanged" }));
    expect(
      await settleUntil(
        () => screen.queryByRole("button", { name: "Office" }),
        "'button', { name: 'Office' }",
      ),
    ).toBeInTheDocument();
  });

  it("shows failed writes reported by OpStatus", async () => {
    const { env } = renderApp();
    const rows = await ready();
    env.transport.data.behaviour.opError = "auth_failed";
    fireEvent.click(rows[1] as HTMLElement);
    fireEvent.keyDown(rows[1] as HTMLElement, { key: "u" });
    expect(
      await settleUntil(
        () => screen.queryByText("A change could not be saved: The server rejected the login."),
        "'A change could not be saved: The server rejected the login.'",
      ),
    ).toBeInTheDocument();
  });
});
