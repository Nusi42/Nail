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
  McpRateLimitIssue,
  McpRateLimits,
  McpServerInfo,
  McpSettings,
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
  "GetMcpSettings",
  "UpdateMcpSettings",
  "UpdateMcpRateLimits",
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
  /** MCP settings (M2c: a fixed port). */
  settings: McpSettings;
  nextClient: number;
  nextAudit: number;
}

const DEMO_TIME = "2026-09-28T09:00:00Z";

/** The rate limits a client starts with (`McpRateLimits::default`, M4a). */
export const DEFAULT_RATE_LIMITS: McpRateLimits = {
  requests_per_minute: 120,
  writes_per_minute: 20,
  sends_per_hour: 10,
};

/** Problems with rate limits (`McpRateLimits::issues`). */
export function rateLimitIssues(limits: McpRateLimits): McpRateLimitIssue[] {
  const values = [limits.requests_per_minute, limits.writes_per_minute, limits.sends_per_hour];
  const issues: McpRateLimitIssue[] = [];
  if (values.includes(0)) issues.push("zero");
  if (values.some((v) => v !== null && v > 10_000)) issues.push("too-high");
  return issues;
}

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
    rate_limits: { ...DEFAULT_RATE_LIMITS },
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
    // The core's Fluent id for the detail (ADR 0046).
    ...(outcome === "denied"
      ? {
          localized: {
            id: "mcp-core-denied-account",
            args: { account: "acc-personal" },
          },
        }
      : {}),
  });
  return {
    clients: [demo],
    audit: [
      entry(1, "search_messages", "allowed"),
      entry(2, "nail://messages/m1", "allowed"),
      entry(3, "nail://accounts/acc-personal/folders", "denied"),
    ],
    server: { running: true, url: "http://127.0.0.1:4781/mcp" },
    settings: { port: null },
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
        rate_limits: { ...DEFAULT_RATE_LIMITS },
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
    case "GetMcpSettings":
      return {
        type: "McpSettings",
        settings: structuredClone(s.settings),
        info: structuredClone(s.server),
      };
    case "UpdateMcpRateLimits": {
      const issue = rateLimitIssues(request.limits)[0];
      if (issue !== undefined) {
        return {
          type: "Error",
          code: "invalid_request",
          message: `invalid rate limits (mcp-rate-limit-issue-${issue})`,
          localized: { id: `mcp-rate-limit-issue-${issue}` },
        };
      }
      const client = s.clients.find((c) => c.id === request.client_id);
      if (!client) return errorResponse("not_found", `MCP client ${request.client_id} not found`);
      if (client.revoked_at !== null) {
        return errorResponse("conflict", `MCP client ${client.id} is revoked`);
      }
      client.rate_limits = structuredClone(request.limits);
      core.emit({ type: "McpClientsChanged" });
      return { type: "McpClientSaved", client: structuredClone(client) };
    }
    case "UpdateMcpSettings": {
      const port = request.settings.port;
      if (port !== null && port < 1024) {
        return {
          type: "Error",
          code: "invalid_request",
          message: "invalid MCP settings (mcp-settings-issue-port)",
          localized: { id: "mcp-settings-issue-port" },
        };
      }
      s.settings = structuredClone(request.settings);
      core.emit({ type: "McpSettingsChanged" });
      return {
        type: "McpSettings",
        settings: structuredClone(s.settings),
        info: structuredClone(s.server),
      };
    }
  }
}
