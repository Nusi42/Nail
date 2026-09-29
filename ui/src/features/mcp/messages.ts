/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/mcp.ftl`; ids built at
 * runtime are listed by `runtimeIds`.
 */
import type { McpCallKind, McpOutcome } from "../../api/generated";
import { READ_SCOPES } from "./queries";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["mcp-"];

/** Literals with those prefixes that are not message ids (part names, widget kinds). */
export const notMessages: readonly string[] = [
  "mcp-clients",
  "mcp-context-indicator",
  "mcp-grants",
  "mcp-audit",
  "mcp-token",
  // The mock core's demo client id.
  "mcp-1",
];

const KINDS: McpCallKind[] = ["resource", "subscribe", "tool", "prompt", "connect"];
const OUTCOMES: McpOutcome[] = ["allowed", "denied", "declined", "error"];
const CLIENT_STATES = ["revoked", "no-access", "granted"];

/** Message ids built at runtime (e.g. `window-<name>` of dialogs). */
export function runtimeIds(): string[] {
  return [
    ...READ_SCOPES.flatMap((s) => [`mcp-scope-${s}`, `mcp-scope-${s}-hint`]),
    ...KINDS.map((k) => `mcp-audit-kind-${k}`),
    ...OUTCOMES.map((o) => `mcp-audit-outcome-${o}`),
    ...CLIENT_STATES.map((s) => `mcp-client-state-${s}`),
  ];
}
