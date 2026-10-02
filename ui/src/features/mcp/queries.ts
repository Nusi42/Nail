/**
 * Server data of the MCP settings (TanStack Query through `NailClient`).
 */
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import type {
  McpAuditEntry,
  McpClientId,
  McpGrant,
  McpRateLimitIssue,
  McpRateLimits,
  McpScope,
} from "../../api/generated";
import { useClient } from "../../state/queries";

/** Audit entries per page. */
export const AUDIT_PAGE = 50;

/** Query keys; everything lives under `mcp` so one invalidation refreshes all. */
export const mcpKeys = {
  all: ["mcp"] as const,
  clients: ["mcp", "clients"] as const,
  server: ["mcp", "server"] as const,
  audit: (clientId: McpClientId | null) => ["mcp", "audit", clientId] as const,
  allAudit: ["mcp", "audit"] as const,
};

/** The reading scopes, in display order. */
export const READ_SCOPES: readonly McpScope[] = [
  "read",
  "raw-source",
  "attachments",
  "encrypted",
  "context",
];

/**
 * The writing scopes (M4a, ADR 0079), in display order. Writes also follow
 * the client's confirmation policy; permanent delete is its own scope and
 * always asks.
 */
export const WRITE_SCOPES: readonly McpScope[] = [
  "organize",
  "permanent-delete",
  "draft",
  "send",
  "calendar",
  "pim",
  "rules",
];

/** Every scope in display order. */
export const ALL_SCOPES: readonly McpScope[] = [...READ_SCOPES, ...WRITE_SCOPES];

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

/** A grant with nothing in it (the state of a new client). */
export function emptyGrant(): McpGrant {
  return {
    scopes: [],
    all_accounts: false,
    accounts: [],
    excluded_folders: [],
    skip_draft_confirmation: false,
  };
}

/** Whether a grant allows nothing (same rule as the core). */
export function grantIsEmpty(grant: McpGrant): boolean {
  return grant.scopes.length === 0 || (!grant.all_accounts && grant.accounts.length === 0);
}

/** `list` with `item` added or removed, in `order` when given. */
export function toggled<T>(list: readonly T[], item: T, on: boolean, order?: readonly T[]): T[] {
  const next = on
    ? list.includes(item)
      ? [...list]
      : [...list, item]
    : list.filter((x) => x !== item);
  return order ? [...next].sort((a, b) => order.indexOf(a) - order.indexOf(b)) : next;
}

export function useMcpClients() {
  const client = useClient();
  return useQuery({
    queryKey: mcpKeys.clients,
    queryFn: async () => (await client.request({ type: "ListMcpClients" }, "McpClients")).clients,
  });
}

export function useMcpServerInfo() {
  const client = useClient();
  return useQuery({
    queryKey: mcpKeys.server,
    queryFn: async () => (await client.request({ type: "GetMcpServerInfo" }, "McpServer")).info,
  });
}

/** The audit log, newest first, paged with `before`. */
export function useMcpAudit(clientId: McpClientId | null) {
  const client = useClient();
  return useInfiniteQuery({
    queryKey: mcpKeys.audit(clientId),
    initialPageParam: null as number | null,
    queryFn: async ({ pageParam }): Promise<McpAuditEntry[]> =>
      (
        await client.request(
          { type: "ListMcpAudit", client_id: clientId, before: pageParam, limit: AUDIT_PAGE },
          "McpAudit",
        )
      ).entries,
    getNextPageParam: (last) =>
      last.length < AUDIT_PAGE ? undefined : (last[last.length - 1]?.id ?? undefined),
  });
}
