/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/mcp.ftl`; ids built at
 * runtime are listed by `runtimeIds`.
 */
import type { McpCallKind, McpOutcome } from "../../api/generated";
import { ALL_SCOPES } from "./queries";
import { RATE_LIMIT_KEYS } from "./RateLimits";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["mcp-"];

/** Literals with those prefixes that are not message ids (part names, widget kinds). */
export const notMessages: readonly string[] = [
  "mcp-clients",
  "mcp-context-indicator",
  "mcp-grants",
  "mcp-audit",
  "mcp-token",
  "mcp-rate-limits",
  // The actor kind of an MCP client (`ActionActor`).
  "mcp-client",
  // The mock core's demo client id.
  "mcp-1",
];

const KINDS: McpCallKind[] = ["resource", "subscribe", "tool", "prompt", "connect"];
const OUTCOMES: McpOutcome[] = ["allowed", "denied", "declined", "error"];
const CLIENT_STATES = ["revoked", "no-access", "granted"];

/** Message ids built at runtime (e.g. `window-<name>` of dialogs). */
export function runtimeIds(): string[] {
  return [
    ...ALL_SCOPES.flatMap((s) => [`mcp-scope-${s}`, `mcp-scope-${s}-hint`]),
    ...RATE_LIMIT_KEYS.flatMap((k) => {
      const id = {
        requests_per_minute: "requests",
        writes_per_minute: "writes",
        sends_per_hour: "sends",
      }[k];
      return [`mcp-rate-${id}`, `mcp-rate-${id}-hint`, `mcp-rate-${id}-unlimited`];
    }),
    "mcp-rate-limit-issue-zero",
    "mcp-rate-limit-issue-too-high",
    ...KINDS.map((k) => `mcp-audit-kind-${k}`),
    ...OUTCOMES.map((o) => `mcp-audit-outcome-${o}`),
    ...CLIENT_STATES.map((s) => `mcp-client-state-${s}`),
  ];
}
