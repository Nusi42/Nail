/**
 * Queries of the plug-in feature. Everything lives under `["plugins"]`, so
 * `PluginsChanged` refreshes it all.
 */
import { useQuery } from "@tanstack/react-query";
import type { PluginInfo, PluginLogEntry } from "../../api/generated";
import { useClient } from "../../state/queries";

/** Lines the log viewer loads. */
export const LOG_LIMIT = 500;

export const pluginKeys = {
  all: ["plugins"] as const,
  list: ["plugins", "list"] as const,
  logs: (pluginId: string) => ["plugins", "logs", pluginId] as const,
  settings: (pluginId: string) => ["plugins", "settings", pluginId] as const,
  panels: ["plugins", "panel"] as const,
  panel: (panel: string, contextKey: string) => ["plugins", "panel", panel, contextKey] as const,
};

/** The installed plug-ins (empty when the core cannot list them). */
export function usePlugins() {
  const client = useClient();
  return useQuery({
    queryKey: pluginKeys.list,
    queryFn: async (): Promise<PluginInfo[]> =>
      (await client.request({ type: "ListPlugins" }, "Plugins")).plugins,
  });
}

/** The log of one plug-in, oldest first. */
export function usePluginLogs(pluginId: string) {
  const client = useClient();
  return useQuery({
    queryKey: pluginKeys.logs(pluginId),
    queryFn: async (): Promise<PluginLogEntry[]> =>
      (
        await client.request(
          { type: "ListPluginLogs", plugin_id: pluginId, after_seq: null, limit: LOG_LIMIT },
          "PluginLogs",
        )
      ).entries,
  });
}

/** Appends a live log line to a loaded log (keeps at most `LOG_LIMIT`). */
export function appendLogLine(
  entries: readonly PluginLogEntry[] | undefined,
  entry: PluginLogEntry,
): PluginLogEntry[] | undefined {
  if (!entries) return entries;
  if (entries.some((e) => e.seq === entry.seq)) return [...entries];
  return [...entries, entry].slice(-LOG_LIMIT);
}

/** A plug-in's settings schema and values. */
export function usePluginSettings(pluginId: string) {
  const client = useClient();
  return useQuery({
    queryKey: pluginKeys.settings(pluginId),
    queryFn: async () =>
      await client.request({ type: "GetPluginSettings", plugin_id: pluginId }, "PluginSettings"),
  });
}
