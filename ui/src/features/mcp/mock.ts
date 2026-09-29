/**
 * The mock core's answers to the MCP requests (WP23), used by
 * `MockTransport` (`../../api/mock.ts` routes exactly these requests here).
 * State lives in `data.features.mcp` ([`createMcpMockState`]). Behaves
 * like `nail-mcp`'s `admin` module: tokens are shown once (only a hint is
 * kept), names are unique among active clients, revoked clients keep their
 * audit entries, every change emits `McpClientsChanged`.
 */
import type {
  McpAuditEntry,
  McpClient,
  McpGrant,
  McpServerInfo,
  Request,
  Response,
} from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";

/** The requests this feature answers. */
export const MCP_REQUESTS = [
  "ListMcpClients",
  "CreateMcpClient",
  "UpdateMcpGrants",
  "RevokeMcpClient",
  "ListMcpAudit",
  "GetMcpServerInfo",
] as const satisfies readonly Request["type"][];

export type McpRequest = Extract<Request, { type: (typeof MCP_REQUESTS)[number] }>;

export function isMcpRequest(request: Request): request is McpRequest {
  return (MCP_REQUESTS as readonly string[]).includes(request.type);
}

/** Mock-core state of this feature. */
export interface McpMockState {
  clients: McpClient[];
  audit: McpAuditEntry[];
  server: McpServerInfo;
  nextClient: number;
  nextAudit: number;
}

const DEMO_TIME = "2026-09-28T09:00:00Z";

/** One demo client with a read grant and a few audit entries. */
export function createMcpMockState(): McpMockState {
  const demo: McpClient = {
    id: "mcp-1",
    name: "Claude Desktop",
    created_at: DEMO_TIME,
    last_used_at: "2026-09-29T07:45:00Z",
    token_hint: "x7Qa",
    grant: {
      scopes: ["read", "context"],
      all_accounts: false,
      accounts: ["acc-work"],
      excluded_folders: [],
      skip_draft_confirmation: false,
    },
    revoked_at: null,
  };
  const entry = (id: number, name: string, outcome: McpAuditEntry["outcome"]): McpAuditEntry => ({
    id,
    client_id: demo.id,
    client_name: demo.name,
    at: `2026-09-29T07:4${id}:00Z`,
    kind: name.startsWith("nail://") ? "resource" : "tool",
    name,
    outcome,
    detail: outcome === "denied" ? "account `acc-personal` not granted" : null,
  });
  return {
    clients: [demo],
    audit: [
      entry(1, "search_messages", "allowed"),
      entry(2, "nail://messages/m1", "allowed"),
      entry(3, "nail://accounts/acc-personal/folders", "denied"),
    ],
    server: { running: true, url: "http://127.0.0.1:4781/mcp" },
    nextClient: 2,
    nextAudit: 4,
  };
}

function state(core: MockCore): McpMockState {
  return core.data.features.mcp;
}

function randomToken(): string {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
  let token = "nail_mcp_";
  for (let i = 0; i < 43; i++) token += alphabet[Math.floor(Math.random() * alphabet.length)];
  return token;
}

function unique<T>(items: readonly T[]): T[] {
  return [...new Set(items)];
}

function normalized(grant: McpGrant): McpGrant {
  return {
    ...grant,
    scopes: unique(grant.scopes),
    accounts: unique(grant.accounts),
    excluded_folders: unique(grant.excluded_folders),
  };
}

/**
 * Simulates an agent reading `nail://ui/context` (for demos and tests):
 * writes an audit entry and emits `McpContextAccess` when the client holds
 * the `context` grant.
 */
export function simulateContextRead(core: MockCore, clientId: string, edit = false): boolean {
  const s = state(core);
  const client = s.clients.find((c) => c.id === clientId && c.revoked_at === null);
  if (!client) return false;
  const allowed = client.grant.scopes.includes("context");
  s.audit.push({
    id: s.nextAudit++,
    client_id: client.id,
    client_name: client.name,
    at: new Date().toISOString(),
    kind: "resource",
    name: "nail://ui/context",
    outcome: allowed ? "allowed" : "denied",
    detail: allowed ? null : "scope `context` not granted",
  });
  if (allowed) {
    core.emit({ type: "McpContextAccess", client_id: client.id, client_name: client.name, edit });
  }
  return allowed;
}

export function answerMcp(core: MockCore, request: McpRequest): Response {
  const s = state(core);
  switch (request.type) {
    case "ListMcpClients":
      return { type: "McpClients", clients: structuredClone(s.clients) };
    case "CreateMcpClient": {
      const name = request.name.trim();
      if (name === "" || name.length > 64) {
        return errorResponse("invalid_request", "an MCP client name has 1 to 64 characters");
      }
      if (s.clients.some((c) => c.name === name && c.revoked_at === null)) {
        return errorResponse("conflict", `an MCP client named "${name}" exists`);
      }
      const token = randomToken();
      const client: McpClient = {
        id: `mcp-${s.nextClient++}`,
        name,
        created_at: new Date().toISOString(),
        last_used_at: null,
        token_hint: token.slice(-4),
        grant: normalized(request.grant),
        revoked_at: null,
      };
      s.clients.push(client);
      core.emit({ type: "McpClientsChanged" });
      return { type: "McpClientCreated", client: structuredClone(client), token };
    }
    case "UpdateMcpGrants": {
      const client = s.clients.find((c) => c.id === request.client_id);
      if (!client) return errorResponse("not_found", `MCP client ${request.client_id}`);
      if (client.revoked_at !== null) {
        return errorResponse("conflict", `MCP client ${request.client_id} is revoked`);
      }
      client.grant = normalized(request.grant);
      core.emit({ type: "McpClientsChanged" });
      return { type: "McpClientSaved", client: structuredClone(client) };
    }
    case "RevokeMcpClient": {
      const client = s.clients.find((c) => c.id === request.client_id);
      if (!client) return errorResponse("not_found", `MCP client ${request.client_id}`);
      if (client.revoked_at === null) {
        client.revoked_at = new Date().toISOString();
        client.grant = {
          scopes: [],
          all_accounts: false,
          accounts: [],
          excluded_folders: [],
          skip_draft_confirmation: false,
        };
      }
      core.emit({ type: "McpClientsChanged" });
      return { type: "Done" };
    }
    case "ListMcpAudit": {
      const limit = Math.min(Math.max(request.limit, 1), 500);
      const before = request.before;
      const entries = s.audit
        .filter((e) => request.client_id === null || e.client_id === request.client_id)
        .filter((e) => before === null || e.id < before)
        .sort((a, b) => b.id - a.id)
        .slice(0, limit);
      return { type: "McpAudit", entries: structuredClone(entries) };
    }
    case "GetMcpServerInfo":
      return { type: "McpServer", info: structuredClone(s.server) };
  }
}
