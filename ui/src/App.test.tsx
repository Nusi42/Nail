import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { App, overridesFromUrl } from "./App";
import { appRegistry } from "./features/registry";
import { expectConsole } from "./test/console";
import { defaultTheme } from "./api/mockData";
import { appWindowEachTest, renderApp } from "./test/app";
import { createEnv } from "./test/harness";
import { must } from "./test/must";
import { INBOX } from "./test/mockIds";

appWindowEachTest();

/** The status bar once the app is up to date. */
async function statusBar(): Promise<HTMLElement> {
  return must(
    (await screen.findByText("Up to date")).closest<HTMLElement>('[data-part="status-bar"]'),
  );
}
afterEach(() => {
  vi.useRealTimers();
});

const themeStyle = () => document.head.querySelector<HTMLStyleElement>("style[data-nail-theme]");

describe("App", () => {
  it("renders the rail and the mail view with the unified inbox", async () => {
    const { container, env } = renderApp();
    const favorite = await screen.findByRole("treeitem", { name: /^All inboxes/ });
    const app = container.querySelector('[data-part="app"]');
    expect(app).toHaveAttribute("data-layout", "default");
    // M5b: the rail beside the view host, which shows the mail view.
    expect(screen.getByRole("navigation", { name: "Sections" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Mail view" })).toBeInTheDocument();
    // The built-in "All accounts" virtual account comes first: the unified inbox.
    expect(env.store.getState().activeAccountId).toBe("all");
    expect(env.store.getState().activeFolderId).toBe("all/inbox");
    expect(favorite).toHaveAttribute("aria-selected", "true");
    expect((await screen.findAllByRole("option")).length).toBeGreaterThan(0);
    expect(screen.getByText("Select a message to read it.")).toBeInTheDocument();
    expect(themeStyle()?.textContent).toContain("--nail-color-accent");
    expect(env.store.getState().connection).toBe("connected");
  });

  it("opens a message on click and shows it in the sandboxed reader", async () => {
    renderApp();
    const rows = await screen.findAllByRole("option");
    fireEvent.click(must(rows[0]));
    // The reader's card feed (P2): the conversation's subject on top, the
    // message in its card.
    const feed = await screen.findByRole("feed");
    expect(
      await screen.findByRole("heading", { name: "Compiler notes for Monday" }),
    ).toBeInTheDocument();
    const frame = await waitFor(() =>
      must(feed.querySelector<HTMLElement>('[data-part="message-view"] iframe')),
    );
    // P3 (ADR 0164): measured, never with scripts.
    expect(frame.getAttribute("sandbox")).toBe("allow-same-origin");
  });

  it("navigates with the layout's key bindings (j/k, ctrl+b, g r)", async () => {
    const { container, env } = renderApp(
      createEnv({ state: { connection: "connecting", activeAccountId: "acc-personal" } }),
    );
    const list = await screen.findByRole("listbox", { name: "Messages" });
    await screen.findAllByRole("option");
    list.focus();
    fireEvent.keyDown(list, { key: "j" });
    expect(env.store.getState().selectedMessageId).toBe(`${INBOX}#1`);
    fireEvent.keyDown(list, { key: "j" });
    fireEvent.keyDown(list, { key: "ArrowDown" });
    expect(env.store.getState().selectedMessageId).toBe(`${INBOX}#3`);
    fireEvent.keyDown(list, { key: "k" });
    expect(env.store.getState().selectedMessageId).toBe(`${INBOX}#2`);

    // j/k are list/reader bindings: inside the folder tree they do nothing.
    const inbox = must(
      container.querySelector<HTMLElement>(
        `[data-part="folder-tree.item"][data-folder="${INBOX}"]`,
      ),
    );
    act(() => {
      inbox.focus();
    });
    fireEvent.keyDown(inbox, { key: "j" });
    expect(env.store.getState().selectedMessageId).toBe(`${INBOX}#2`);

    // Global: ctrl+b toggles the sidebar.
    fireEvent.keyDown(document.body, { key: "b", ctrlKey: true });
    expect(container.querySelector('[data-node-id="sidebar"]')).not.toBeInTheDocument();
    fireEvent.keyDown(document.body, { key: "b", ctrlKey: true });
    expect(container.querySelector('[data-node-id="sidebar"]')).toBeInTheDocument();

    // Sequence: g r toggles the reader.
    fireEvent.keyDown(document.body, { key: "g" });
    expect(container.querySelector('[data-node-id="reader"]')).toBeInTheDocument();
    fireEvent.keyDown(document.body, { key: "r" });
    expect(container.querySelector('[data-node-id="reader"]')).not.toBeInTheDocument();
  });

  it("ignores key bindings while typing in a text field", async () => {
    const { env } = renderApp();
    await screen.findAllByRole("option");
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.focus();
    fireEvent.keyDown(input, { key: "j" });
    expect(env.store.getState().selectedMessageId).toBeNull();
    input.remove();
  });

  it("hot-swaps theme and layout on ConfigChanged", async () => {
    const { container, env } = renderApp();
    await screen.findByRole("treeitem", { name: /^All inboxes/ });
    const theme = defaultTheme();
    const layout = env.transport.data.layout;
    // A layout whose main window is the narrow mail window (tabs), no views.
    const narrowOnly = {
      ...layout,
      name: "custom",
      windows: {
        main: [
          layout.windows.mail?.[1] ?? { min_width: null, max_width: null, root: { type: "stack" } },
        ],
      },
    };
    act(() => {
      env.transport.emit({
        type: "ConfigChanged",
        theme: {
          ...theme,
          tokens: { ...theme.tokens, color: { ...theme.tokens.color, accent: "#123456" } },
        },
        layout: narrowOnly as typeof layout,
      });
    });
    await waitFor(() => {
      expect(themeStyle()?.textContent).toContain("--nail-color-accent: #123456;");
    });
    expect(document.head.querySelectorAll("style[data-nail-theme]")).toHaveLength(1);
    expect(container.querySelector('[data-part="app"]')).toHaveAttribute("data-layout", "custom");
    expect(container.querySelector('[data-part="tabs"]')).toBeInTheDocument();
  });

  it("reports config errors and sync progress in the status bar", async () => {
    const { env } = renderApp();
    const status = await statusBar();
    act(() => {
      env.transport.emit({
        type: "ConfigError",
        file: "layout.kdl",
        line: 4,
        column: 2,
        message: "bad",
      });
    });
    expect(status).toHaveTextContent("layout.kdl, line 4, column 2");
    act(() => {
      env.transport.emit({ type: "SyncProgress", account_id: "acc-personal", done: 1, total: 4 });
    });
    expect(env.store.getState().sync).toEqual({ accountId: "acc-personal", done: 1, total: 4 });
  });

  it("reports a failed synchronization until the next one makes progress", async () => {
    const { env } = renderApp();
    const status = await statusBar();
    act(() => {
      env.transport.emit({
        type: "SyncFailed",
        account_id: "acc-personal",
        code: "auth_failed",
        message: "LOGIN rejected",
      });
    });
    expect(status).toHaveTextContent("Synchronization failed: The server rejected the login.");
    expect(status.querySelector('[data-item="sync-progress"]')?.getAttribute("data-state")).toBe(
      "error",
    );
    act(() => {
      env.transport.emit({ type: "SyncProgress", account_id: "acc-personal", done: 4, total: 4 });
    });
    expect(status).toHaveTextContent("Up to date");
    expect(env.store.getState().syncError).toBeNull();
  });

  it("refetches folders and messages on MailChanged", async () => {
    const { env } = renderApp();
    await screen.findAllByRole("option");
    // The default list shows conversations (P2): one row per thread.
    const lists = () =>
      env.transport.requests.filter((r) => r.type === "ListMessages" || r.type === "ListThreads")
        .length;
    const before = lists();
    expect(before).toBeGreaterThan(0);
    act(() => {
      env.transport.emit({ type: "MailChanged", folder_ids: [INBOX] });
    });
    await waitFor(() => {
      expect(lists()).toBeGreaterThan(before);
    });
  });

  it("uses the German locale with English fallback", async () => {
    const env = createEnv({ state: { localeOverride: "de" } });
    renderApp(env);
    expect(await screen.findByRole("listbox", { name: "Messages" })).toBeInTheDocument();
    expect(document.documentElement.lang).toBe("de");
  });

  it("reads overrides from the URL", () => {
    expect(overridesFromUrl("?lang=de&mode=dark")).toEqual({
      localeOverride: "de",
      modeOverride: "dark",
    });
    expect(overridesFromUrl("?mode=neon")).toEqual({ localeOverride: null, modeOverride: null });
  });

  // ADR 0207: the page reports its start, and a failed render shows the
  // error screen (not a white window) and reaches the core's log.
  it("reports its start and catches a failed render", async () => {
    expectConsole(/status bar broke/);
    const env = createEnv();
    const Broken = () => {
      throw new Error("status bar broke");
    };
    render(
      <App
        client={env.client}
        queryClient={env.queryClient}
        store={env.store}
        widgets={{ ...appRegistry.widgets, "status-bar": Broken }}
      />,
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Something in this window failed to show. Reload it to continue.",
    );
    expect(screen.getByRole("button", { name: "Reload" })).toBeInTheDocument();
    const reports = env.transport.requests.flatMap((r) =>
      r.type === "ReportUiDiagnostics" ? [r.report] : [],
    );
    expect(reports[0]).toMatchObject({ type: "started" });
    expect(reports).toContainEqual(
      expect.objectContaining({
        type: "problem",
        kind: "render-error",
        message: "Error: status bar broke",
      }),
    );
  });
});
