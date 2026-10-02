/**
 * Queries of the plug-in feature. Everything lives under `["plugins"]`, so
 * `PluginsChanged` refreshes it all.
 */
import { useQuery } from "@tanstack/react-query";
import type {
  MessageId,
  PluginActivity,
  PluginInfo,
  PluginLogEntry,
  PluginPreview,
} from "../../api/generated";
import { clientIdOf } from "./context";
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
  annotations: (messageId: string) => ["plugins", "annotations", messageId] as const,
  preview: (messageId: string, index: number, renderer: string) =>
    ["plugins", "preview", messageId, index, renderer] as const,
  activity: (pluginId: string) => ["plugins", "activity", pluginId] as const,
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

/** The banners and badges the plug-ins' open hooks show with a message. */
export function useMessageAnnotations(messageId: MessageId | null) {
  const client = useClient();
  return useQuery({
    queryKey: pluginKeys.annotations(messageId ?? ""),
    enabled: messageId !== null,
    staleTime: 60_000,
    queryFn: async () =>
      await client.request(
        {
          type: "GetMessagePluginAnnotations",
          message_id: messageId ?? "",
          client_id: clientIdOf(client),
        },
        "MessagePluginAnnotations",
      ),
  });
}

/** A plug-in's preview of an attachment (`renderer` = `<plugin>.<name>`). */
export function useAttachmentPreview(messageId: MessageId, index: number, renderer: string | null) {
  const client = useClient();
  return useQuery({
    queryKey: pluginKeys.preview(messageId, index, renderer ?? ""),
    enabled: renderer !== null,
    staleTime: Number.POSITIVE_INFINITY,
    queryFn: async (): Promise<PluginPreview> =>
      (
        await client.request(
          {
            type: "RenderAttachmentPreview",
            message_id: messageId,
            index,
            renderer: renderer ?? "",
            client_id: clientIdOf(client),
          },
          "AttachmentPreview",
        )
      ).preview,
  });
}

/** A plug-in's jobs, hooks and CPU use. */
export function usePluginActivity(pluginId: string) {
  const client = useClient();
  return useQuery({
    queryKey: pluginKeys.activity(pluginId),
    queryFn: async (): Promise<PluginActivity> =>
      (await client.request({ type: "GetPluginActivity", plugin_id: pluginId }, "PluginActivity"))
        .activity,
  });
}
