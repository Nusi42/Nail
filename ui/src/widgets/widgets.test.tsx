import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { normalizeLayout } from "../api/normalize";
import { createEnv, renderNode, stubElementSizes, widget } from "../test/harness";
import { flattenFolders } from "./FolderTree";
import { compileRows, rowStates } from "./MessageList";

const INBOX = "acc-personal/Inbox";

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
});
afterEach(() => restoreSizes());

describe("account-switcher", () => {
  it("lists accounts and selects one", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal" } });
    renderNode(widget("account-switcher"), env);
    const personal = await screen.findByRole("button", { name: "Personal" });
    const work = screen.getByRole("button", { name: "Work" });
    expect(personal).toHaveAttribute("data-part", "account-switcher.item");
    expect(personal).toHaveAttribute("aria-pressed", "true");
    expect(personal).toHaveAttribute("data-state", "selected");
    fireEvent.click(work);
    expect(env.store.getState().activeAccountId).toBe("acc-work");
    expect(work).toHaveAttribute("data-state", "selected");
    expect(screen.getByRole("group", { name: "Accounts" })).toHaveAttribute(
      "data-part",
      "account-switcher",
    );
  });
});

describe("folder-tree", () => {
  it("renders the folder hierarchy with roles and unread counts", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    const { container } = renderNode(
      widget("folder-tree", { options: { "show-unread": true } }),
      env,
    );
    const inbox = await screen.findByRole("treeitem", { name: /Inbox/ });
    expect(inbox).toHaveAttribute("data-role", "inbox");
    expect(inbox).toHaveAttribute("aria-selected", "true");
    expect(inbox).toHaveAttribute("data-state", "selected");
    expect(within(inbox).getByText("10")).toHaveAttribute(
      "data-part",
      "folder-tree.item.unread-count",
    );
    expect(within(inbox).getByLabelText("10 unread messages")).toBeInTheDocument();
    const receipts = screen.getByRole("treeitem", { name: /Receipts/ });
    expect(receipts).toHaveAttribute("aria-level", "2");
    expect(receipts).toHaveAttribute("data-depth", "1");
    expect(
      container.querySelector('[data-part="folder-tree.item.icon"][data-icon="inbox"]'),
    ).toBeInTheDocument();
    expect(
      container.querySelector(
        '[data-folder="acc-personal/Drafts"] [data-part="folder-tree.item.unread-count"]',
      ),
    ).toBeNull();

    fireEvent.click(receipts);
    expect(env.store.getState().activeFolderId).toBe("acc-personal/Inbox/Receipts");
    // Special folders come first in a fixed order: Inbox (and its
    // children), Drafts, Sent, Archive, Junk, Trash.
    fireEvent.keyDown(receipts, { key: "ArrowDown" });
    expect(env.store.getState().activeFolderId).toBe("acc-personal/Drafts");
    fireEvent.keyDown(receipts, { key: "Home" });
    expect(env.store.getState().activeFolderId).toBe(INBOX);
  });

  it("names special folders by role (raw name as tooltip) and lists them first (M5b review)", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    // A server that names its folders in capitals, in alphabetical order.
    const raw = env.transport.data.folders
      .filter((f) => f.account_id === "acc-personal")
      .map((f) => (f.role === "other" ? f : { ...f, name: f.name.toUpperCase() }));
    raw.sort((a, b) => a.name.localeCompare(b.name));
    env.transport.data.folders = [
      ...env.transport.data.folders.filter((f) => f.account_id !== "acc-personal"),
      ...raw,
    ];
    const { container } = renderNode(widget("folder-tree"), env);
    await screen.findByRole("treeitem", { name: /^Inbox/ });
    const names = [...container.querySelectorAll('[data-part="folder-tree.item.name"]')];
    expect(names.map((n) => n.textContent)).toEqual([
      "Inbox",
      "Receipts",
      "Drafts",
      "Sent",
      "Archive",
      "Junk",
      "Trash",
    ]);
    expect(names[0]).toHaveAttribute("title", "INBOX");
    expect(names[1]).not.toHaveAttribute("title");
  });

  it("hides unread counts when show-unread is false", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal" } });
    const { container } = renderNode(
      widget("folder-tree", { options: { "show-unread": false } }),
      env,
    );
    await screen.findByRole("treeitem", { name: /Inbox/ });
    expect(container.querySelector('[data-part="folder-tree.item.unread-count"]')).toBeNull();
  });

  it("orders children after their parents", () => {
    const f = (id: string, parent: string | null) => ({
      id,
      account_id: "a",
      parent_id: parent,
      name: id,
      path: id,
      role: "other" as const,
      unread: 0,
      total: 0,
    });
    expect(
      flattenFolders([f("b", "a"), f("a", null), f("c", null), f("orphan", "gone")]).map(
        (r) => `${r.folder.id}:${r.depth}`,
      ),
    ).toEqual(["a:0", "b:1", "c:0", "orphan:0"]);
  });
});

describe("message-list", () => {
  const node = () =>
    widget("message-list", {
      options: { "page-size": 50 },
      rows: [
        "{from.name | bold-if-unread}  {date | relative}",
        "{subject | truncate(12)}",
        "{preview}",
      ],
    });

  it("renders a virtualized window of rows from templates", async () => {
    const env = createEnv({ inboxSize: 250, state: { activeFolderId: INBOX } });
    const { container } = renderNode(node(), env);
    const rows = await screen.findAllByRole("option");
    // 600px viewport / 40px rows + overscan — far fewer than 50 loaded rows.
    expect(rows.length).toBeGreaterThan(5);
    expect(rows.length).toBeLessThan(40);
    const first = rows[0] as HTMLElement;
    expect(first).toHaveAttribute("data-part", "message-list.row");
    expect(first).toHaveAttribute("data-state", "unread flagged answered has-attachments");
    expect(first.querySelectorAll('[data-part="message-list.row.line"]')).toHaveLength(3);
    const from = first.querySelector('[data-part="message-list.row.from"]');
    expect(from).toHaveTextContent("Grace Hopper");
    expect(from).toHaveAttribute("data-emphasis", "strong");
    expect(first.querySelector('[data-part="message-list.row.date"]')?.textContent).not.toBe("");
    expect(first.querySelector('[data-part="message-list.row.subject"]')).toHaveTextContent(
      "Compiler not…",
    );
    expect(first.querySelector('[data-part="message-list.row.preview"]')).toHaveTextContent(
      /^Hello Ada/,
    );
    expect(container.querySelectorAll('[data-part="message-list.spacer"]')).toHaveLength(2);
    expect(env.store.getState().messageOrder).toHaveLength(50);
    const listbox = screen.getByRole("listbox", { name: "Messages" });
    expect(listbox).toHaveAttribute("data-key-context", "list");
  });

  it("selects a row on click", async () => {
    const env = createEnv({ state: { activeFolderId: INBOX } });
    renderNode(node(), env);
    const rows = await screen.findAllByRole("option");
    fireEvent.click(rows[1] as HTMLElement);
    expect(env.store.getState().selectedMessageId).toBe(`${INBOX}#2`);
    expect(rows[1]).toHaveAttribute("aria-selected", "true");
    expect((rows[1] as HTMLElement).dataset.state?.split(" ")).toContain("selected");
    expect(screen.getByRole("listbox")).toHaveAttribute(
      "aria-activedescendant",
      (rows[1] as HTMLElement).id,
    );
  });

  it("loads further pages as the end comes into view", async () => {
    const env = createEnv({ inboxSize: 30, state: { activeFolderId: INBOX } });
    renderNode(widget("message-list", { options: { "page-size": 10 } }), env);
    await waitFor(() => expect(env.store.getState().messageOrder).toHaveLength(30));
    const pages = env.transport.requests.filter((r) => r.type === "ListMessages");
    expect(pages.map((r) => (r.type === "ListMessages" ? r.offset : -1))).toEqual([0, 10, 20]);
  });

  it("shows an empty state", async () => {
    const env = createEnv({ state: { activeFolderId: "acc-personal/Drafts" } });
    renderNode(node(), env);
    expect(await screen.findByText("No messages in this folder.")).toHaveAttribute(
      "data-part",
      "message-list.empty",
    );
  });

  it("shows a localized error", async () => {
    const env = createEnv({ state: { activeFolderId: "nope" } });
    renderNode(node(), env);
    expect(await screen.findByText("The item no longer exists.")).toHaveAttribute(
      "data-part",
      "message-list.error",
    );
  });

  it("falls back to the default rows for invalid templates", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    expect(compileRows(["{subject | nope}"]).map((t) => t.source)).toEqual([
      "{from.name | bold-if-unread}  {date | relative}",
      "{subject}",
    ]);
    expect(compileRows(["{subject}", "{x | nope}"]).map((t) => t.source)).toEqual(["{subject}"]);
    expect(warn).toHaveBeenCalledTimes(2);
    warn.mockRestore();
  });

  it("derives row states", () => {
    const flags = { seen: true, flagged: false, answered: false, draft: false };
    const base = {
      id: "m",
      folder_id: "f",
      account_id: "a",
      subject: "",
      from: null,
      to: [],
      date: "",
      flags,
      has_attachments: false,
      preview: "",
      thread_id: null,
      tags: [],
      duplicates: [],
    };
    expect(rowStates(base, false)).toBeUndefined();
    expect(rowStates({ ...base, flags: { ...flags, seen: false } }, true)).toBe("selected unread");
  });
});

describe("message-view", () => {
  it("shows an empty state without a selection", () => {
    const env = createEnv();
    renderNode(widget("message-view"), env);
    expect(screen.getByText("Select a message to read it.")).toHaveAttribute(
      "data-part",
      "message-view.empty",
    );
  });

  it("renders headers and the HTML body in a sandboxed iframe with a CSP", async () => {
    const env = createEnv({ state: { selectedMessageId: `${INBOX}#1` } });
    const { container } = renderNode(
      widget("message-view", { options: { headers: "from cc date" } }),
      env,
    );
    expect(
      await screen.findByRole("heading", { name: "Compiler notes for Monday" }),
    ).toHaveAttribute("data-part", "message-view.header.subject");
    const fields = [...container.querySelectorAll('[data-part="message-view.header.field"]')].map(
      (f) => f.getAttribute("data-field"),
    );
    expect(fields).toEqual(["from", "cc", "date"]);
    expect(container.querySelector('[data-field="from"]')).toHaveTextContent(
      "FromGrace Hopper <grace@navy.example>",
    );
    const iframe = container.querySelector("iframe") as HTMLIFrameElement;
    expect(iframe).toHaveAttribute("data-part", "message-view.body");
    expect(iframe.getAttribute("sandbox")).toBe("");
    const doc = iframe.getAttribute("srcdoc") ?? "";
    expect(doc).toContain('http-equiv="Content-Security-Policy"');
    expect(doc).toContain("default-src 'none'");
    expect(doc).toContain("<b>Ada</b>");
    expect(iframe).toHaveAttribute("title", "Message content: Compiler notes for Monday");
    expect(screen.getByRole("list", { name: "Attachments" })).toHaveTextContent("report.pdf");
    expect(screen.getByRole("article", { name: "Message" })).toHaveAttribute(
      "data-key-context",
      "reader",
    );
  });

  it("falls back to plain text", async () => {
    const env = createEnv({ state: { selectedMessageId: `${INBOX}#2` } });
    const { container } = renderNode(widget("message-view"), env);
    await screen.findByRole("heading");
    expect(container.querySelector("iframe")).toBeNull();
    const text = container.querySelector('[data-part="message-view.text"]');
    expect(text?.tagName).toBe("PRE");
    expect(text).toHaveTextContent(/Hello Ada/);
    expect(screen.queryByRole("note")).toBeNull();
  });

  it("shows a localized error for missing messages", async () => {
    const env = createEnv({ state: { selectedMessageId: "gone" } });
    renderNode(widget("message-view"), env);
    expect(await screen.findByText("The item no longer exists.")).toHaveAttribute(
      "data-part",
      "message-view.error",
    );
  });
});

describe("toolbar", () => {
  it("renders items from the layout with i18n labels, icons and disabled states", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal" } });
    // The default reader bar (M5b: ranked, "More", hide-disabled), shown here
    // with disabled actions and wrapping so jsdom (no widths) sees every item.
    const base = env.transport.data.layout;
    const layout = normalizeLayout({
      ...base,
      toolbar_settings: {
        ...base.toolbar_settings,
        reader: { hide_disabled: false, overflow: "wrap" },
      },
    });
    const { container } = renderNode(widget("toolbar", { options: { toolbar: "reader" } }), env, {
      layout,
    });
    const toolbar = screen.getByRole("toolbar", { name: "Message actions" });
    const buttons = within(toolbar).getAllByRole("button");
    const names = buttons.map((b) => b.getAttribute("aria-label") ?? b.textContent);
    expect(names).toEqual([
      "Reply",
      "Reply all",
      "Forward",
      "Archive",
      "Delete",
      "Move to…",
      "Flag",
      "Snooze…",
      "More",
    ]);
    expect(buttons[0]).toHaveAttribute("data-part", "toolbar.button");
    expect(buttons[0]).toBeDisabled(); // no message selected
    expect(buttons[0]).toHaveAttribute("data-state", "disabled");
    expect(buttons[0]?.querySelector('[data-part="toolbar.button.icon"]')).toHaveAttribute(
      "data-icon",
      "reply",
    );
    // Icon buttons keep their label as tooltip, with the key.
    expect(within(toolbar).getByRole("button", { name: "Archive" })).toHaveAttribute(
      "title",
      "Archive (E)",
    );
    expect(container.querySelectorAll('[data-part="toolbar.spacer"]')).toHaveLength(1);
    expect(container.querySelectorAll('[data-part="toolbar.separator"]')).toHaveLength(1);
  });

  it("hides the reader bar's disabled actions (hide-disabled)", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal" } });
    renderNode(widget("toolbar", { options: { toolbar: "reader" } }), env);
    const toolbar = screen.getByRole("toolbar", { name: "Message actions" });
    // Nothing selected: every message action is disabled, so none shows.
    expect(within(toolbar).queryByRole("button", { name: "Reply" })).toBeNull();
    expect(within(toolbar).queryByRole("button", { name: "Archive" })).toBeNull();
  });

  it("dispatches enabled actions", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-work" } });
    const layout = normalizeLayout({
      ...env.transport.data.layout,
      toolbars: {
        main: [
          { type: "action", action: "sync.now" },
          { type: "action", action: "view.toggle", args: ["sidebar"], label: "Sidebar" },
        ],
      },
    });
    renderNode(widget("toolbar", { options: { toolbar: "main" } }), env, { layout });
    fireEvent.click(screen.getByRole("button", { name: "Check for new mail" }));
    await waitFor(() =>
      expect(env.transport.requests).toContainEqual({ type: "SyncNow", account_id: "acc-work" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Sidebar" }));
    expect(env.store.getState().hiddenNodes).toEqual(["sidebar"]);
  });
});

describe("status-bar", () => {
  it("retries a failed write with RetryOp", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal" } });
    env.transport.data.failedOps.set("op-9", "acc-personal");
    renderNode(widget("status-bar", { options: { items: "ops" } }), env);
    act(() => {
      env.store.getState().opQueued(["op-9"]);
      env.store.getState().opFinished("op-9", "unavailable");
    });
    const status = screen.getByRole("status", { name: "Status" });
    expect(within(status).getByText(/A change could not be saved/)).toBeInTheDocument();
    fireEvent.click(within(status).getByRole("button", { name: "Retry" }));
    await waitFor(() =>
      expect(env.transport.requests).toContainEqual({ type: "RetryOp", op_id: "op-9" }),
    );
    await waitFor(() => expect(env.store.getState().opError).toBeNull());
    expect(within(status).queryByRole("button", { name: "Retry" })).toBeNull();
  });

  it("shows sync progress, folder counts and connection problems", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    renderNode(widget("status-bar", { options: { items: "sync-progress folder-counts" } }), env);
    const status = screen.getByRole("status", { name: "Status" });
    expect(status).toHaveAttribute("data-part", "status-bar");
    expect(within(status).getByText("Up to date")).toHaveAttribute("data-state", "idle");
    expect(await within(status).findByText("30 messages, 10 unread")).toHaveAttribute(
      "data-item",
      "folder-counts",
    );

    act(() => env.store.getState().setSync({ accountId: "acc-personal", done: 2, total: 5 }));
    expect(within(status).getByText("Syncing… 2 of 5")).toHaveAttribute("data-state", "syncing");
    act(() =>
      env.store.getState().setConfigError({ file: "theme.kdl", line: 3, column: 7, message: "x" }),
    );
    expect(within(status).getByText(/theme\.kdl, line 3, column 7/)).toHaveAttribute(
      "data-state",
      "error",
    );
    act(() => env.store.getState().setConnection("reconnecting"));
    expect(within(status).getByText("Offline – reconnecting…")).toBeInTheDocument();
  });
});

describe("text and spacer", () => {
  it("renders literal and @id text", () => {
    const env = createEnv();
    const { container } = renderNode(
      normalizeLayout({
        windows: {
          main: [
            {
              root: {
                type: "stack",
                children: [
                  { type: "widget", kind: "text", options: { text: "@app.name" } },
                  { type: "widget", kind: "spacer" },
                  { type: "widget", kind: "text", options: { text: "Plain" } },
                ],
              },
            },
          ],
        },
      }).windows.main?.[0]?.root ?? widget("text"),
      env,
    );
    const texts = container.querySelectorAll('[data-part="text"]');
    expect([...texts].map((t) => t.textContent)).toEqual(["Nail", "Plain"]);
    expect(container.querySelector('[data-part="spacer"]')).toHaveAttribute("aria-hidden", "true");
  });
});
