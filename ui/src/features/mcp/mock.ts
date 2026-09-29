/**
 * The mock core's answers to the MCP requests (WP23), used by
 * `MockTransport` (`../../api/mock.ts` routes exactly these requests here).
 * State lives in `data.features.mcp` ([`createMcpMockState`]).
 */
import type { Request, Response } from "../../api/generated";
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
export type McpMockState = Record<string, never>;

export function createMcpMockState(): McpMockState {
  return {};
}

export function answerMcp(core: MockCore, request: McpRequest): Response {
  void core;
  return errorResponse("unsupported", `${request.type} arrives with M1b WP23`);
}
