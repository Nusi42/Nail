/**
 * The mock core's answers to the M4a plug-in extension requests (ADR
 * 0079), used by `MockTransport` (`../../api/mock.ts` routes exactly these
 * requests here).
 *
 * Like the core at the contract stage: activity is the manifest's jobs
 * with no runs yet (and no hook calls); open hooks contribute no
 * annotations; previews, actions and manual job runs answer
 * `unsupported`; nothing was vetoed, so "Send anyway" answers `not_found`.
 * WP104 extends this with the simulated host for its UI tests.
 */
import type { PluginActivity, Request, Response } from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";

/** The requests this module answers. */
export const PLUGIN_EXTENSION_REQUESTS = [
  "GetPluginActivity",
  "RunPluginJob",
  "GetMessagePluginAnnotations",
  "RenderAttachmentPreview",
  "RunPluginAction",
  "OverrideSendVeto",
] as const satisfies readonly Request["type"][];

export type PluginExtensionRequest = Extract<
  Request,
  { type: (typeof PLUGIN_EXTENSION_REQUESTS)[number] }
>;

export function isPluginExtensionRequest(request: Request): request is PluginExtensionRequest {
  return (PLUGIN_EXTENSION_REQUESTS as readonly string[]).includes(request.type);
}

/** CPU time a plug-in may use per hour (`CPU_QUOTA_MS_PER_HOUR`). */
export const CPU_QUOTA_MS_PER_HOUR = 120_000;

export function answerPluginExtensions(core: MockCore, request: PluginExtensionRequest): Response {
  switch (request.type) {
    case "GetPluginActivity": {
      const plugin = core.data.features.plugins.plugins.find((p) => p.id === request.plugin_id);
      if (!plugin) return errorResponse("not_found", `plug-in not found: ${request.plugin_id}`);
      const activity: PluginActivity = {
        plugin_id: plugin.id,
        jobs: (plugin.manifest.contributes?.jobs ?? []).map((job) => ({
          job: job.id,
          title: job.title ?? null,
          schedule: job.schedule ?? null,
          events: job.events ?? [],
          running: false,
          last_run_at: null,
          last_outcome: null,
          last_error: null,
          next_run_at: null,
          run_count: 0,
          failure_count: 0,
        })),
        hooks: [],
        cpu_ms_last_hour: 0,
        cpu_quota_ms_per_hour: CPU_QUOTA_MS_PER_HOUR,
      };
      return { type: "PluginActivity", activity };
    }
    case "GetMessagePluginAnnotations":
      return {
        type: "MessagePluginAnnotations",
        message_id: request.message_id,
        banners: [],
        badges: [],
      };
    case "RunPluginJob":
    case "RenderAttachmentPreview":
    case "RunPluginAction":
      return errorResponse("unsupported", `${request.type} is not wired yet (M4a WP105)`);
    case "OverrideSendVeto":
      return errorResponse("not_found", `no send veto ${request.veto_id} is pending`);
  }
}
