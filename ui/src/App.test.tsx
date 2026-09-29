import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { App, overridesFromUrl } from "./App";
import { defaultTheme } from "./api/mockData";
import { createEnv, setWindowWidth, stubElementSizes } from "./test/harness";

const INBOX = "acc-personal/Inbox";

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
  setWindowWidth(1200);
});
afterEach(() => {
  vi.useRealTimers();
  restoreSizes();
  setWindowWidth(1024);
});

function renderApp(env = createEnv({ state: { connection: "connecting" } })) {
  const result = render(
    <App client={env.client} queryClient={env.queryClient} store={env.store} />,
  );
  return { ...result, env };
}

const themeStyle = () => document.head.querySelector<HTMLStyleElement>("style[data-nail-theme]");

describe("App", () => {
  it("renders the default three-pane UI from the mock core", async () => {
    const { container, env } = renderApp();
    await screen.findByRole("treeitem", { name: /Inbox/ });
    const app = container.querySelector('[data-part="app"]');
    expect(app).toHaveAttribute("data-layout", "default");
    expect(env.store.getState().activeAccountId).toBe("acc-personal");
    expect(env.store.getState().activeFolderId).toBe(INBOX);
    expect((await screen.findAllByRole("option")).length).toBeGreaterThan(0);
    expect(screen.getByText("Select a message to read it.")).toBeInTheDocument();
    expect(themeStyle()?.textContent).toContain("--nail-color-accent");
    expect(env.store.getState().connection).toBe("connected");
  });

  it("opens a message on click and shows it in the sandboxed reader", async () => {
    const { container } = renderApp();
    const rows = await screen.findAllByRole("option");
    fireEvent.click(rows[0] as HTMLElement);
    const view = container.querySelector('[data-part="message-view"]') as HTMLElement;
    expect(
      await within(view).findByRole("heading", { name: "Compiler notes for Monday" }),
    ).toBeInTheDocument();
    expect(view.querySelector("iframe")?.getAttribute("sandbox")).toBe("");
  });

  it("navigates with the layout's key bindings (j/k, ctrl+b, g r)", async () => {
    const { container, env } = renderApp();
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
    const inbox = screen.getByRole("treeitem", { name: /Inbox/ });
    inbox.focus();
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
    await screen.findByRole("treeitem", { name: /Inbox/ });
    const theme = defaultTheme();
    const layout = env.transport.data.layout;
    const narrowOnly = {
      ...layout,
      name: "custom",
      windows: {
        main: [
          layout.windows.main?.[1] ?? { min_width: null, max_width: null, root: { type: "stack" } },
        ],
      },
    };
    act(() =>
      env.transport.emit({
        type: "ConfigChanged",
        theme: {
          ...theme,
          tokens: { ...theme.tokens, color: { ...theme.tokens.color, accent: "#123456" } },
        },
        layout: narrowOnly as typeof layout,
      }),
    );
    await waitFor(() =>
      expect(themeStyle()?.textContent).toContain("--nail-color-accent: #123456;"),
    );
    expect(document.head.querySelectorAll("style[data-nail-theme]")).toHaveLength(1);
    expect(container.querySelector('[data-part="app"]')).toHaveAttribute("data-layout", "custom");
    expect(container.querySelector('[data-part="tabs"]')).toBeInTheDocument();
  });

  it("reports config errors and sync progress in the status bar", async () => {
    const { env } = renderApp();
    const status = (await screen.findByText("Up to date")).closest(
      '[data-part="status-bar"]',
    ) as HTMLElement;
    act(() =>
      env.transport.emit({
        type: "ConfigError",
        file: "layout.kdl",
        line: 4,
        column: 2,
        message: "bad",
      }),
    );
    expect(status).toHaveTextContent("layout.kdl, line 4, column 2");
    act(() =>
      env.transport.emit({ type: "SyncProgress", account_id: "acc-personal", done: 1, total: 4 }),
    );
    expect(env.store.getState().sync).toEqual({ accountId: "acc-personal", done: 1, total: 4 });
  });

  it("reports a failed synchronization until the next one makes progress", async () => {
    const { env } = renderApp();
    const status = (await screen.findByText("Up to date")).closest(
      '[data-part="status-bar"]',
    ) as HTMLElement;
    act(() =>
      env.transport.emit({
        type: "SyncFailed",
        account_id: "acc-personal",
        code: "auth_failed",
        message: "LOGIN rejected",
      }),
    );
    expect(status).toHaveTextContent("Synchronization failed: The server rejected the login.");
    expect(status.querySelector('[data-item="sync-progress"]')?.getAttribute("data-state")).toBe(
      "error",
    );
    act(() =>
      env.transport.emit({ type: "SyncProgress", account_id: "acc-personal", done: 4, total: 4 }),
    );
    expect(status).toHaveTextContent("Up to date");
    expect(env.store.getState().syncError).toBeNull();
  });

  it("refetches folders and messages on MailChanged", async () => {
    const { env } = renderApp();
    await screen.findAllByRole("option");
    const before = env.transport.requests.filter((r) => r.type === "ListMessages").length;
    act(() => env.transport.emit({ type: "MailChanged", folder_ids: [INBOX] }));
    await waitFor(() =>
      expect(
        env.transport.requests.filter((r) => r.type === "ListMessages").length,
      ).toBeGreaterThan(before),
    );
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
});
