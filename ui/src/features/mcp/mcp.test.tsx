import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { McpClient, Request } from "../../api/generated";
import { createEnv, renderNode, renderWithProviders, widget, type Env } from "../../test/harness";
import { handleMcpEvent } from "./index";
import { runtimeIds } from "./messages";
import { answerMcp, createMcpMockState, simulateContextRead } from "./mock";
import { clientState } from "./McpClients";
import { emptyGrant, grantIsEmpty, toggled } from "./queries";
import { INDICATOR_MS, mcpStore } from "./state";
import { readerSelection, REPORT_DELAY_MS, UiContextReporter } from "./UiContextReporter";
import { must } from "../../test/must";

/** Feeds core events to the feature's event handler, as the app shell does. */
function wireEvents(env: Env) {
  return env.client.subscribe((event) => {
    handleMcpEvent(event, { client: env.client, queryClient: env.queryClient, store: env.store });
  });
}

function requestsOf<T extends Request["type"]>(env: Env, type: T) {
  return env.transport.requests.filter((r): r is Extract<Request, { type: T }> => r.type === type);
}

afterEach(() => {
  mcpStore.getState().clear();
  vi.useRealTimers();
});

describe("mcp-clients", () => {
  it("creates a client and shows its token exactly once", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    wireEvents(env);
    renderNode(widget("mcp-clients"), env);

    const list = await screen.findByRole("list", { name: "MCP clients" });
    expect(within(list).getByText("Claude Desktop")).toBeInTheDocument();
    expect(
      screen.getByText("The MCP server listens on http://127.0.0.1:4781/mcp"),
    ).toBeInTheDocument();

    await user.type(screen.getByRole("textbox", { name: "Client name" }), "  Cursor ");
    await user.click(screen.getByRole("button", { name: "Create client" }));

    const panel = await screen.findByRole("region", { name: "Token for “Cursor”" });
    const token = within(panel).getByText(/^nail_mcp_/).textContent;
    expect(token).toHaveLength("nail_mcp_".length + 43);
    expect(within(panel).getByText(/cannot show it again/)).toBeInTheDocument();
    expect(requestsOf(env, "CreateMcpClient")[0]).toMatchObject({
      name: "Cursor",
      grant: emptyGrant(),
    });

    // The new client is listed, selected and has no access.
    const item = await within(list).findByRole("button", { name: /Cursor/ });
    expect(item).toHaveAttribute("aria-pressed", "true");
    expect(item).toHaveAttribute("data-access", "no-access");
    expect(within(item).getByText(`Token …${token.slice(-4)}`)).toBeInTheDocument();
    expect(screen.getByText(/This client has no access/)).toBeInTheDocument();

    await user.click(within(panel).getByRole("button", { name: "Done" }));
    expect(screen.queryByText(token)).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: /Token for/ })).not.toBeInTheDocument();
  });

  it("refuses a duplicate name", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("mcp-clients"), env);
    await screen.findByRole("list", { name: "MCP clients" });
    await user.type(screen.getByRole("textbox", { name: "Client name" }), "Claude Desktop");
    await user.click(screen.getByRole("button", { name: "Create client" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "A client with this name already exists.",
    );
    expect(screen.queryByRole("region", { name: /Token for/ })).not.toBeInTheDocument();
  });

  it("grants scopes, accounts and hidden folders", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    wireEvents(env);
    renderNode(widget("mcp-clients"), env);
    const clients = await screen.findByRole("list", { name: "MCP clients" });
    await user.type(screen.getByRole("textbox", { name: "Client name" }), "Agent");
    await user.click(screen.getByRole("button", { name: "Create client" }));
    const form = await screen.findByRole("form", { name: "Access of Agent" });
    const save = within(form).getByRole("button", { name: "Save access" });
    expect(save).toBeDisabled();

    await user.click(within(form).getByLabelText("Original message source"));
    await user.click(within(form).getByLabelText("Read mail"));
    await user.click(within(form).getByLabelText("Work (ada@work.example)"));
    // Folders of the granted account can be hidden.
    const folders = await within(form).findByText(/^Work/, { selector: "summary" });
    await user.click(folders);
    const hide = await within(form).findAllByLabelText(/^Hide /);
    await user.click(must(hide[0]));
    expect(save).toBeEnabled();
    await user.click(save);

    expect(await within(form).findByRole("status")).toHaveTextContent("Access saved.");
    const update = requestsOf(env, "UpdateMcpGrants")[0];
    expect(update?.grant.scopes).toEqual(["read", "raw-source"]);
    expect(update?.grant.accounts).toEqual(["acc-work"]);
    expect(update?.grant.all_accounts).toBe(false);
    expect(update?.grant.excluded_folders).toHaveLength(1);
    await waitFor(() =>
      expect(within(clients).getByRole("button", { name: /Agent/, pressed: true })).toHaveAttribute(
        "data-access",
        "granted",
      ),
    );
    expect(screen.queryByText(/This client has no access/)).not.toBeInTheDocument();

    // All accounts replaces the account list.
    await user.click(within(form).getByLabelText(/All accounts/));
    expect(within(form).queryByLabelText("Work (ada@work.example)")).not.toBeInTheDocument();
  });

  it("revokes a client after confirmation", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    wireEvents(env);
    renderNode(widget("mcp-clients"), env);
    await screen.findByRole("list", { name: "MCP clients" });
    await user.click(await screen.findByRole("button", { name: "Revoke" }));
    expect(requestsOf(env, "RevokeMcpClient")).toHaveLength(0);
    expect(screen.getByText(/Revoke “Claude Desktop”\?/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Revoke client" }));

    await waitFor(() =>
      expect(screen.getByRole("button", { name: /Claude Desktop/ })).toHaveAttribute(
        "data-access",
        "revoked",
      ),
    );
    expect(requestsOf(env, "RevokeMcpClient")[0]).toMatchObject({ client_id: "mcp-1" });
    expect(screen.queryByRole("button", { name: "Revoke" })).not.toBeInTheDocument();
    const form = screen.getByRole("form", { name: "Access of Claude Desktop" });
    expect(within(form).getByRole("checkbox", { name: "Read mail" })).toBeDisabled();
  });

  it("shows the audit log per client and for all clients", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    wireEvents(env);
    renderNode(widget("mcp-clients"), env);
    const log = await screen.findByRole("region", { name: "MCP activity" });
    const rows = await within(log).findAllByRole("row");
    expect(rows).toHaveLength(4); // header + 3 demo entries, newest first
    expect(rows[1]).toHaveAttribute("data-outcome", "denied");
    expect(within(must(rows[1])).getByText("Denied")).toBeInTheDocument();
    // The core's localized text, not its English diagnostic.
    const detail = within(must(rows[1])).getByText(
      /refused: the account .acc-personal. is not granted/,
    );
    expect(detail).toHaveAttribute("data-part", "mcp-audit.detail");
    expect(
      within(must(rows[1])).queryByText("account `acc-personal` not granted"),
    ).not.toBeInTheDocument();
    expect(requestsOf(env, "ListMcpAudit")).toContainEqual(
      expect.objectContaining({ client_id: "mcp-1", before: null }),
    );

    // A context read by the agent shows up after the event.
    act(() => {
      simulateContextRead(env.transport, "mcp-1");
    });
    await waitFor(() => {
      expect(within(log).getAllByRole("row")).toHaveLength(5);
    });

    await user.click(screen.getByRole("checkbox", { name: "Show activity of all clients" }));
    await waitFor(() => {
      expect(requestsOf(env, "ListMcpAudit").some((r) => r.client_id === null)).toBe(true);
    });
  });
});

describe("mcp-context-indicator", () => {
  it("appears while an agent reads the screen and hides again", () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const env = createEnv();
    wireEvents(env);
    renderNode(widget("mcp-context-indicator"), env);
    const status = screen.getByRole("status", { name: "Agent activity" });
    expect(status).toHaveAttribute("data-state", "idle");
    expect(status).toBeEmptyDOMElement();

    act(() => {
      expect(simulateContextRead(env.transport, "mcp-1")).toBe(true);
    });
    expect(status).toHaveAttribute("data-state", "reading");
    expect(status).toHaveTextContent("Claude Desktop is reading what is on your screen");

    act(() => {
      vi.advanceTimersByTime(INDICATOR_MS - 100);
    });
    expect(status).toHaveAttribute("data-state", "reading");
    // A second access restarts the timer.
    act(() => {
      env.transport.emit({
        type: "McpContextAccess",
        client_id: "mcp-1",
        client_name: "Claude Desktop",
        edit: true,
      });
    });
    expect(status).toHaveTextContent("Claude Desktop is editing your draft");
    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(status).toHaveAttribute("data-state", "editing");
    act(() => {
      vi.advanceTimersByTime(INDICATOR_MS);
    });
    expect(status).toHaveAttribute("data-state", "idle");
  });

  it("can be dismissed and ignores clients without the context grant", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    wireEvents(env);
    renderNode(widget("mcp-context-indicator"), env);
    const status = screen.getByRole("status", { name: "Agent activity" });
    const created = answerMcp(env.transport, {
      type: "CreateMcpClient",
      name: "No context",
      grant: emptyGrant(),
    });
    expect(created.type).toBe("McpClientCreated");
    act(() => {
      expect(simulateContextRead(env.transport, "mcp-2")).toBe(false);
    });
    expect(status).toHaveAttribute("data-state", "idle");

    act(() => {
      simulateContextRead(env.transport, "mcp-1");
    });
    await user.click(screen.getByRole("button", { name: "Hide agent indicator" }));
    expect(status).toHaveAttribute("data-state", "idle");
  });
});

describe("UI context reporting", () => {
  it("reports what is open, once per change", async () => {
    const env = createEnv({
      state: { activeAccountId: "acc-work", activeFolderId: "f-work-inbox" },
    });
    renderWithProviders(<UiContextReporter />, env);
    await waitFor(() => {
      expect(requestsOf(env, "ReportUiContext")).toHaveLength(1);
    });
    expect(requestsOf(env, "ReportUiContext")[0]?.context).toEqual({
      account_id: "acc-work",
      folder_id: "f-work-inbox",
      message_id: null,
      thread_id: null,
      selected_message_ids: [],
      selected_text: null,
      draft_id: null,
      draft_cursor: null,
    });

    act(() => {
      env.store.getState().selectMessage("m-1");
    });
    await waitFor(() => {
      expect(requestsOf(env, "ReportUiContext")).toHaveLength(2);
    });
    expect(requestsOf(env, "ReportUiContext")[1]?.context).toMatchObject({
      message_id: "m-1",
      selected_message_ids: ["m-1"],
    });
    expect(env.transport.data.uiContext.message_id).toBe("m-1");

    // Nothing changed: no new report.
    act(() => {
      env.store.getState().setKeyContext("reader");
    });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, REPORT_DELAY_MS * 2));
    });
    expect(requestsOf(env, "ReportUiContext")).toHaveLength(2);
  });

  it("takes selected text only from the message reader", () => {
    document.body.innerHTML =
      '<div data-part="message-view.text"><p id="in">Quarterly plan</p></div><p id="out">Other</p>';
    const select = (id: string) => {
      const range = document.createRange();
      range.selectNodeContents(must(document.getElementById(id)));
      const selection = must(document.getSelection());
      selection.removeAllRanges();
      selection.addRange(range);
      return selection;
    };
    expect(readerSelection(select("in"))).toBe("Quarterly plan");
    expect(readerSelection(select("out"))).toBeNull();
    expect(readerSelection(null)).toBeNull();
    document.getSelection()?.removeAllRanges();
    document.body.innerHTML = "";
  });
});

describe("mcp mock and helpers", () => {
  it("behaves like the core's admin requests", () => {
    const env = createEnv();
    const core = env.transport;
    const events: string[] = [];
    core.onEvent((e) => events.push(e.type));

    const created = answerMcp(core, { type: "CreateMcpClient", name: "A", grant: emptyGrant() });
    if (created.type !== "McpClientCreated") throw new Error(created.type);
    expect(created.client.token_hint).toBe(created.token.slice(-4));
    expect(
      answerMcp(core, { type: "CreateMcpClient", name: "A", grant: emptyGrant() }),
    ).toMatchObject({
      code: "conflict",
    });
    expect(
      answerMcp(core, { type: "CreateMcpClient", name: " ", grant: emptyGrant() }),
    ).toMatchObject({
      code: "invalid_request",
    });

    const saved = answerMcp(core, {
      type: "UpdateMcpGrants",
      client_id: created.client.id,
      grant: { ...emptyGrant(), scopes: ["read", "read"], all_accounts: true },
    });
    expect(saved).toMatchObject({ client: { grant: { scopes: ["read"], all_accounts: true } } });

    expect(answerMcp(core, { type: "RevokeMcpClient", client_id: created.client.id })).toEqual({
      type: "Done",
    });
    expect(
      answerMcp(core, {
        type: "UpdateMcpGrants",
        client_id: created.client.id,
        grant: emptyGrant(),
      }),
    ).toMatchObject({ code: "conflict" });
    expect(answerMcp(core, { type: "RevokeMcpClient", client_id: "mcp-99" })).toMatchObject({
      code: "not_found",
    });
    expect(events.filter((e) => e === "McpClientsChanged")).toHaveLength(3);

    const page = answerMcp(core, { type: "ListMcpAudit", client_id: null, before: 3, limit: 1 });
    expect(page).toMatchObject({ entries: [{ id: 2 }] });
    expect(answerMcp(core, { type: "GetMcpServerInfo" })).toMatchObject({
      info: { running: true },
    });
  });

  it("derives client states and edits grants", () => {
    const client: McpClient = must(createMcpMockState().clients[0]);
    expect(clientState(client)).toBe("granted");
    expect(clientState({ ...client, grant: emptyGrant() })).toBe("no-access");
    expect(clientState({ ...client, revoked_at: "2026-01-01T00:00:00Z" })).toBe("revoked");
    expect(grantIsEmpty({ ...emptyGrant(), scopes: ["read"] })).toBe(true);
    expect(grantIsEmpty({ ...emptyGrant(), scopes: ["read"], all_accounts: true })).toBe(false);
    expect(toggled(["context"], "read", true, ["read", "context"])).toEqual(["read", "context"]);
    expect(toggled(["read", "context"], "read", false)).toEqual(["context"]);
    expect(toggled(["read"], "read", true)).toEqual(["read"]);
    expect(runtimeIds()).toContain("mcp-scope-raw-source-hint");
  });
});
