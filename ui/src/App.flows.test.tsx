/**
 * M1a flows through the whole app (default layout, key bindings, windows,
 * events) against the mock core.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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

async function ready() {
  return screen.findAllByRole("option");
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
    const dialog = await screen.findByRole("dialog", { name: "New message" });
    expect(dialog).toHaveAttribute("data-window", "composer");
    await waitFor(() =>
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
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    const status = screen.getByRole("status");
    expect(within(status).getByText(/Sending in (10|9) seconds/)).toBeInTheDocument();
    await user.click(within(status).getByRole("button", { name: "Undo" }));
    const reopened = await screen.findByRole("dialog", { name: "New message" });
    expect(within(reopened).getByRole("textbox", { name: "Subject" })).toHaveValue("Lunch");
    expect(env.transport.data.outbox).toEqual([]);

    // Escape closes the window; the message stays for later.
    fireEvent.keyDown(within(reopened).getByRole("textbox", { name: "Subject" }), {
      key: "Escape",
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
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
    const status = screen.getByRole("status");
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

  it("archives with e, moves with v and marks flags with s", async () => {
    const user = userEvent.setup();
    const { env } = renderApp();
    const rows = await ready();
    const first = rows[0] as HTMLElement;
    const firstId = first.dataset.message ?? "";
    fireEvent.click(first);
    fireEvent.keyDown(first, { key: "s" });
    await waitFor(() =>
      expect(env.transport.requests).toContainEqual(
        expect.objectContaining({ type: "UpdateFlags", message_ids: [firstId] }),
      ),
    );
    fireEvent.keyDown(document.body, { key: "e" });
    await waitFor(() =>
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
    const picker = await screen.findByRole("dialog", { name: "Move to folder" });
    await user.click(await within(picker).findByRole("button", { name: "Trash" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(env.transport.requests).toContainEqual({
      type: "MoveMessages",
      message_ids: [next],
      target: { folder_id: "all/trash" },
    });
  });

  it("searches with / and the query language, and clears with Escape", async () => {
    const user = userEvent.setup();
    const { env } = renderApp();
    await ready();
    fireEvent.keyDown(document.body, { key: "/" });
    const input = screen.getByRole("searchbox", { name: "Search mail" });
    expect(input).toHaveFocus();
    await user.type(input, "from:grace{Enter}");
    await waitFor(() =>
      expect(screen.getByRole("listbox", { name: "Messages" })).toHaveAttribute(
        "data-state",
        "search",
      ),
    );
    const hits = await screen.findAllByRole("option");
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
    const settings = await screen.findByRole("dialog", { name: "Settings" });
    expect(within(settings).getByRole("tab", { name: "Accounts" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await user.click(within(settings).getByRole("tab", { name: "Virtual accounts" }));
    expect(await within(settings).findByText(/No virtual accounts yet/)).toBeVisible();
    await user.click(within(toolbar).getByRole("button", { name: "Add account" }));
    const setup = await screen.findByRole("dialog", { name: "Add account" });
    expect(setup).toHaveAttribute("data-state", "active");
    expect(settings).not.toHaveAttribute("data-state");
    await user.click(within(setup).getByRole("button", { name: "Close" }));
    await waitFor(() => expect(settings).toHaveAttribute("data-state", "active"));
  });

  it("applies account status and account changes pushed by the core", async () => {
    const { env } = renderApp();
    await ready();
    const work = await screen.findByRole("button", { name: "Work" });
    await waitFor(() => expect(work).toHaveAttribute("data-status", "online"));
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
    await waitFor(() => expect(work).toHaveAttribute("data-status", "offline"));
    expect(screen.getByRole("status")).toHaveTextContent("Work: Offline");

    env.transport.data.accounts[1] = {
      ...(env.transport.data.accounts[1] as (typeof env.transport.data.accounts)[number]),
      name: "Office",
    };
    act(() => env.transport.emit({ type: "AccountsChanged" }));
    expect(await screen.findByRole("button", { name: "Office" })).toBeInTheDocument();
  });

  it("shows failed writes reported by OpStatus", async () => {
    const { env } = renderApp();
    const rows = await ready();
    env.transport.data.behaviour.opError = "auth_failed";
    fireEvent.click(rows[1] as HTMLElement);
    fireEvent.keyDown(rows[1] as HTMLElement, { key: "u" });
    expect(
      await screen.findByText("A change could not be saved: The server rejected the login."),
    ).toBeInTheDocument();
  });
});
