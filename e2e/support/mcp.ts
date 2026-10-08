import { expect, type Locator, type Page } from "@playwright/test";
import { defined } from "./defined";
import { openSettingsTab } from "./views";
/**
 * A minimal MCP client over Streamable HTTP (JSON-RPC over POST; answers
 * come as JSON or as a server-sent event stream), enough for the e2e
 * scenarios. Real agents use an MCP SDK or `nail mcp --client`.
 */

export interface JsonRpcResponse {
  jsonrpc: "2.0";
  id: number;
  result?: Record<string, unknown>;
  error?: { code: number; message: string; data?: Record<string, unknown> };
}

export class McpSession {
  private nextId = 1;
  private session: string | null = null;

  constructor(
    private readonly url: string,
    private readonly token: string,
  ) {}

  private headers(): Record<string, string> {
    const headers: Record<string, string> = {
      authorization: `Bearer ${this.token}`,
      "content-type": "application/json",
      accept: "application/json, text/event-stream",
    };
    if (this.session) headers["mcp-session-id"] = this.session;
    return headers;
  }

  /** Sends a request and returns its response. */
  async request(method: string, params: Record<string, unknown> = {}): Promise<JsonRpcResponse> {
    const id = this.nextId++;
    const response = await fetch(this.url, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify({ jsonrpc: "2.0", id, method, params }),
    });
    if (!response.ok)
      throw new Error(`${method}: HTTP ${String(response.status)} ${await response.text()}`);
    this.session = response.headers.get("mcp-session-id") ?? this.session;
    if ((response.headers.get("content-type") ?? "").includes("text/event-stream")) {
      return readEvent(response, id);
    }
    return (await response.json()) as JsonRpcResponse;
  }

  /** Sends a notification (no response expected). */
  async notify(method: string, params: Record<string, unknown> = {}): Promise<void> {
    const response = await fetch(this.url, {
      method: "POST",
      headers: this.headers(),
      body: JSON.stringify({ jsonrpc: "2.0", method, params }),
    });
    await response.body?.cancel();
  }

  /** `initialize` + `notifications/initialized`. */
  async open(): Promise<JsonRpcResponse> {
    const init = await this.request("initialize", {
      protocolVersion: "2025-06-18",
      capabilities: {},
      clientInfo: { name: "nail-e2e", version: "1.0.0" },
    });
    await this.notify("notifications/initialized");
    return init;
  }
}

/** Reads server-sent events until the JSON-RPC response with `id`. */
async function readEvent(response: Response, id: number): Promise<JsonRpcResponse> {
  const reader = defined(response.body, "the response body").getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      let end: number;
      while ((end = buffer.indexOf("\n\n")) >= 0) {
        const event = buffer.slice(0, end);
        buffer = buffer.slice(end + 2);
        const data = event
          .split("\n")
          .filter((line) => line.startsWith("data:"))
          .map((line) => line.slice(5).trimStart())
          .join("\n");
        if (data === "") continue;
        const message = JSON.parse(data) as JsonRpcResponse;
        if (message.id === id) return message;
      }
    }
  } finally {
    await reader.cancel().catch(() => undefined);
  }
  throw new Error(`no response for request ${String(id)}`);
}

/** An MCP client created in Settings → MCP clients. */
export interface McpClient {
  /** The settings view. */
  settings: Locator;
  /** The server's endpoint, as the settings show it. */
  url: string;
  /** The client's token (shown once). */
  token: string;
  /** The client's access form. */
  access: Locator;
}

/**
 * Opens Settings → MCP clients, waits for the running MCP server and
 * creates the client `name` (no access yet).
 */
export async function createMcpClient(page: Page, name: string): Promise<McpClient> {
  const settings = await openSettingsTab(page, "MCP clients");
  // The server runs; its URL is shown.
  const serverLine = settings.locator('[data-part="mcp-clients.server"]');
  await expect(serverLine).toHaveAttribute("data-state", "running");
  const url = /(http:\/\/127\.0\.0\.1:\d+\/mcp)/.exec((await serverLine.textContent()) ?? "")?.[1];
  // A new client's token is shown once.
  await settings.getByRole("textbox", { name: "Client name" }).fill(name);
  await settings.getByRole("button", { name: "Create client" }).click();
  const token = (await settings.locator('[data-part="mcp-token.value"]').textContent())?.trim();
  expect(token).toMatch(/^nail_mcp_/);
  return {
    settings,
    url: defined(url, "the MCP URL"),
    token: defined(token, "the token"),
    access: settings.getByRole("form", { name: `Access of ${name}` }),
  };
}

/** Grants the permissions `scopes` (their labels) for all accounts and saves. */
export async function grantMcpAccess(access: Locator, scopes: readonly string[]): Promise<void> {
  for (const scope of scopes) {
    await access.getByRole("checkbox", { name: scope, exact: true }).check();
  }
  await access.getByRole("checkbox", { name: /All accounts/ }).check();
  await access.getByRole("button", { name: "Save access" }).click();
  await expect(access.getByRole("status")).toHaveText("Access saved.");
}
