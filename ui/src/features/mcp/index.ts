/**
 * Feature slot of M1b WP23 (MCP): MCP client settings (tokens, grants,
 * revoke, audit log), the indicator shown while an agent reads the UI
 * context, and the reporting of that context to the core.
 *
 * Only WP23 edits the files in this directory. The central registry
 * (`../registry.ts`) merges these exports into the app; see `../README.md`.
 */
import type { Feature, FeatureEventHandler } from "../types";
import { ContextIndicator } from "./ContextIndicator";
import { McpClients } from "./McpClients";
import { mcpKeys } from "./queries";
import { mcpStore } from "./state";
import { UiContextReporter } from "./UiContextReporter";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "mcp-clients": McpClients,
  "mcp-context-indicator": ContextIndicator,
};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  /** Hides the context indicator. */
  "mcp.indicator.dismiss": {
    run: () => mcpStore.getState().clear(),
    enabled: () => mcpStore.getState().access !== null,
  },
};

/** Named dialogs (opened with `window.open "<name>"`). */
export const dialogs: Feature["dialogs"] = {};

/** Refreshes the MCP settings and drives the indicator. */
export const handleMcpEvent: FeatureEventHandler = (event, { queryClient }) => {
  switch (event.type) {
    case "McpClientsChanged":
      void queryClient.invalidateQueries({ queryKey: mcpKeys.all });
      break;
    case "McpContextAccess":
      mcpStore.getState().noteAccess(event.client_id, event.client_name, event.edit);
      void queryClient.invalidateQueries({ queryKey: mcpKeys.allAudit });
      break;
  }
};

/** Handlers for core events. */
export const events: Feature["events"] = [handleMcpEvent];

/** Background components mounted once in the app shell. */
export const effects: Feature["effects"] = [UiContextReporter];
