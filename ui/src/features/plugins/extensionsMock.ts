/**
 * The mock core's answers to the M4a plug-in extension requests (ADR
 * 0079), used by `MockTransport` (`../../api/mock.ts` routes exactly these
 * requests here), plus the before-send check of `SendDraft`
 * (`answerSendDraft`, for the integrator to route `SendDraft` through).
 *
 * It simulates what the host does once WP102 / WP105 are wired: plug-ins
 * register an `ExtensionBehaviour` (open / before-send hooks, preview
 * renderers, composer / attachment / link actions, jobs) in
 * `data.features.plugins.extensions.behaviours`; requests check the
 * plug-in runs, holds the grant and declares the contribution, like the
 * core. Hook calls, job runs and CPU time are counted for
 * `GetPluginActivity`. Without a behaviour a plug-in contributes nothing
 * (annotations empty, previews / actions `not_found`, jobs answer `Done`
 * and record a run). With `host: null` the host-backed requests answer
 * `unsupported`, as the core did at the contract stage.
 */
import type {
  AttachmentInfo,
  Draft,
  MessageId,
  PluginActionTarget,
  PluginActivity,
  PluginBadgeSpec,
  PluginBannerSpec,
  PluginHookKind,
  PluginHookStats,
  PluginInfo,
  PluginJobOutcome,
  PluginJobStatus,
  PluginPermission,
  PluginPreview,
  PluginSendVeto,
  Request,
  Response,
  SendCrypto,
} from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";
import { sendDraft } from "../../api/mockOutbox";
import { extensionId, matchesFile, MAX_PREVIEW_INPUT, parseExtensionId } from "./extensions";
import { stateOf } from "./mock";
import { pluginApi, type MockPluginApi } from "./mockHost";

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
/** How long a send veto can be overridden (ms; the core keeps 10 minutes). */
export const VETO_TTL_MS = 10 * 60_000;

/** What a simulated plug-in does at each extension point. */
export interface ExtensionBehaviour {
  /** Open hook: banners and badges for a message. */
  open?: (message: { id: MessageId; subject: string; from: string | null }) => {
    banners?: PluginBannerSpec[];
    badges?: PluginBadgeSpec[];
  };
  /** Before-send hook: a veto with the plug-in's reason, or nothing. */
  beforeSend?: (draft: Draft) => { veto?: { reason: string } } | undefined;
  /** Preview renderers by name. */
  previews?: Record<string, (attachment: AttachmentInfo, messageId: MessageId) => PluginPreview>;
  /** Composer / attachment / link actions by name. */
  actions?: Record<
    string,
    (
      api: MockPluginApi,
      target: PluginActionTarget,
      clientId: string | null,
      core: MockCore,
    ) => void
  >;
  /** Jobs by name; the outcome of a run (default `ok`). */
  jobs?: Record<string, () => PluginJobOutcome | undefined>;
}

type JobState = Omit<PluginJobStatus, "job" | "title" | "schedule" | "events">;

/** Counters of one plug-in. */
export interface ExtensionActivity {
  jobs: Record<string, JobState>;
  hooks: Partial<Record<PluginHookKind, PluginHookStats>>;
  cpuMs: number;
}

interface PendingVeto {
  draftId: string;
  delaySecs: number | null;
  sendAt: string | null;
  crypto?: SendCrypto;
  until: number;
}

/** Mock-core state of the extension points (in `data.features.plugins.extensions`). */
export interface ExtensionsMockState {
  behaviours: Record<string, ExtensionBehaviour>;
  activity: Record<string, ExtensionActivity>;
  vetoes: Map<string, PendingVeto>;
  nextVeto: number;
}

export function createExtensionsMockState(): ExtensionsMockState {
  return { behaviours: {}, activity: {}, vetoes: new Map(), nextVeto: 1 };
}

function activityOf(core: MockCore, pluginId: string): ExtensionActivity {
  const state = core.data.features.plugins.extensions;
  return (state.activity[pluginId] ??= { jobs: {}, hooks: {}, cpuMs: 0 });
}

/** Counts one hook call (like `record_plugin_hook_call`). */
export function recordHookCall(
  core: MockCore,
  pluginId: string,
  hook: PluginHookKind,
  outcome: "ok" | "vetoed" | "failed" | "timed-out",
  ms: number,
  error: string | null = null,
): void {
  const activity = activityOf(core, pluginId);
  const stats = (activity.hooks[hook] ??= {
    hook,
    calls: 0,
    failures: 0,
    timeouts: 0,
    vetoes: 0,
    total_ms: 0,
    max_ms: 0,
    last_error: null,
    last_at: null,
  });
  stats.calls += 1;
  if (outcome === "failed") stats.failures += 1;
  if (outcome === "timed-out") stats.timeouts += 1;
  if (outcome === "vetoed") stats.vetoes += 1;
  stats.total_ms += ms;
  stats.max_ms = Math.max(stats.max_ms, ms);
  if (error !== null) stats.last_error = error;
  stats.last_at = new Date().toISOString();
  activity.cpuMs += ms;
}

function running(core: MockCore, plugin: PluginInfo): boolean {
  return plugin.enabled && stateOf(core.data.features.plugins, plugin) === "running";
}

function granted(plugin: PluginInfo, permission: PluginPermission, accountId?: string | null) {
  const grant = plugin.grants.find((g) => g.permission === permission);
  return (
    grant !== undefined &&
    (grant.accounts === null || accountId == null || grant.accounts.includes(accountId))
  );
}

function declaresHook(plugin: PluginInfo, hook: PluginHookKind): boolean {
  return (plugin.manifest.contributes?.hooks ?? []).some((h) => h.hook === hook);
}

const HOOK_PERMISSION: Record<PluginHookKind, PluginPermission> = {
  arrival: "hooks.arrival",
  beforeSend: "hooks.beforeSend",
  open: "hooks.open",
};

/** Plug-ins whose `hook` runs (running, granted, declared, with a behaviour). */
function hooked(core: MockCore, hook: PluginHookKind, accountId?: string | null) {
  const state = core.data.features.plugins;
  return state.plugins
    .filter(
      (p) =>
        running(core, p) && granted(p, HOOK_PERMISSION[hook], accountId) && declaresHook(p, hook),
    )
    .map((p) => ({ plugin: p, behaviour: state.extensions.behaviours[p.id] }));
}

function accountOfMessage(core: MockCore, messageId: string): string | null {
  return core.data.messages.find((m) => m.id === messageId)?.account_id ?? null;
}

function findMessage(core: MockCore, messageId: string) {
  return core.data.messages.find((m) => m.id === messageId);
}

function annotations(core: MockCore, messageId: string): Response {
  const summary = findMessage(core, messageId);
  const body = core.data.bodies.get(messageId);
  if (!summary && !body) return errorResponse("not_found", `message ${messageId} not found`);
  const message = {
    id: messageId,
    subject: summary?.subject ?? body?.headers.subject ?? "",
    from: summary?.from?.email ?? body?.headers.from[0]?.email ?? null,
  };
  const banners: Extract<Response, { type: "MessagePluginAnnotations" }>["banners"] = [];
  const badges: Extract<Response, { type: "MessagePluginAnnotations" }>["badges"] = [];
  for (const { plugin, behaviour } of hooked(core, "open", accountOfMessage(core, messageId))) {
    if (!behaviour?.open) continue;
    try {
      const result = behaviour.open(message);
      // The core keeps at most three of each (`PluginOpenResult::problems`).
      for (const banner of (result.banners ?? []).slice(0, 3)) {
        banners.push({ plugin_id: plugin.id, plugin_name: plugin.manifest.name, banner });
      }
      for (const badge of (result.badges ?? []).slice(0, 3)) {
        badges.push({ plugin_id: plugin.id, plugin_name: plugin.manifest.name, badge });
      }
      recordHookCall(core, plugin.id, "open", "ok", 4);
    } catch (error) {
      recordHookCall(core, plugin.id, "open", "failed", 4, String(error));
    }
  }
  return { type: "MessagePluginAnnotations", message_id: messageId, banners, badges };
}

/**
 * The before-send hooks for `SendDraft`: `SendVetoed` when a plug-in
 * vetoes (nothing is queued), else `null` (send as usual).
 */
export function checkSendVeto(
  core: MockCore,
  request: Extract<Request, { type: "SendDraft" }>,
): Response | null {
  const draft = core.data.drafts.get(request.draft_id);
  if (!draft) return null;
  const accountId =
    core.data.accountsConfig.accounts.find((a) =>
      a.identities.some((i) => i.id === draft.identity_id),
    )?.id ?? null;
  const vetoes: PluginSendVeto[] = [];
  for (const { plugin, behaviour } of hooked(core, "beforeSend", accountId)) {
    if (!behaviour?.beforeSend) continue;
    try {
      const veto = behaviour.beforeSend(structuredClone(draft))?.veto;
      if (veto) {
        vetoes.push({
          plugin_id: plugin.id,
          plugin_name: plugin.manifest.name,
          reason: veto.reason,
        });
        recordHookCall(core, plugin.id, "beforeSend", "vetoed", 6);
      } else {
        recordHookCall(core, plugin.id, "beforeSend", "ok", 6);
      }
    } catch (error) {
      // A failing hook never blocks sending.
      recordHookCall(core, plugin.id, "beforeSend", "failed", 6, String(error));
    }
  }
  if (vetoes.length === 0) return null;
  const state = core.data.features.plugins.extensions;
  const vetoId = `veto-${String(state.nextVeto++)}`;
  state.vetoes.set(vetoId, {
    draftId: request.draft_id,
    delaySecs: request.delay_secs,
    sendAt: request.send_at,
    ...(request.crypto ? { crypto: request.crypto } : {}),
    until: Date.now() + VETO_TTL_MS,
  });
  return { type: "SendVetoed", draft_id: request.draft_id, veto_id: vetoId, vetoes };
}

/** `SendDraft` with the before-send hooks (route `SendDraft` here). */
export function answerSendDraft(
  core: MockCore,
  request: Extract<Request, { type: "SendDraft" }>,
): Response {
  return (
    checkSendVeto(core, request) ??
    sendDraft(core, request.draft_id, request.delay_secs, request.send_at, request.crypto)
  );
}

function jobStatus(core: MockCore, plugin: PluginInfo): PluginJobStatus[] {
  const activity = activityOf(core, plugin.id);
  return (plugin.manifest.contributes?.jobs ?? []).map((job) => ({
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
    ...activity.jobs[job.id],
  }));
}

function activityResponse(core: MockCore, plugin: PluginInfo): PluginActivity {
  const activity = activityOf(core, plugin.id);
  const order: PluginHookKind[] = ["arrival", "beforeSend", "open"];
  return {
    plugin_id: plugin.id,
    jobs: jobStatus(core, plugin),
    hooks: order.flatMap((h) => (activity.hooks[h] ? [structuredClone(activity.hooks[h])] : [])),
    cpu_ms_last_hour: activity.cpuMs,
    cpu_quota_ms_per_hour: CPU_QUOTA_MS_PER_HOUR,
  };
}

function runJob(core: MockCore, plugin: PluginInfo, job: string): Response {
  const def = (plugin.manifest.contributes?.jobs ?? []).find((j) => j.id === job);
  if (!def) return errorResponse("not_found", `job ${job} not found in ${plugin.id}`);
  if (!running(core, plugin)) return errorResponse("unavailable", `${plugin.id} is not running`);
  const activity = activityOf(core, plugin.id);
  const current = activity.jobs[job];
  if (current?.running) return errorResponse("conflict", `job ${job} is running`);
  const quotaLeft = activity.cpuMs < CPU_QUOTA_MS_PER_HOUR;
  let outcome: PluginJobOutcome = "cpu-quota";
  let error: string | null = null;
  if (quotaLeft) {
    try {
      outcome =
        core.data.features.plugins.extensions.behaviours[plugin.id]?.jobs?.[job]?.() ?? "ok";
    } catch (e) {
      outcome = "error";
      error = String(e);
    }
    activity.cpuMs += 35;
  }
  const failed = outcome !== "ok";
  activity.jobs[job] = {
    running: false,
    last_run_at: new Date().toISOString(),
    last_outcome: outcome,
    last_error: error,
    next_run_at: current?.next_run_at ?? null,
    run_count: (current?.run_count ?? 0) + 1,
    failure_count: (current?.failure_count ?? 0) + (failed ? 1 : 0),
  };
  core.emit({ type: "PluginActivityChanged", plugin_id: plugin.id });
  return { type: "Done" };
}

function preview(
  core: MockCore,
  request: Extract<PluginExtensionRequest, { type: "RenderAttachmentPreview" }>,
): Response {
  const parsed = parseExtensionId(request.renderer);
  const plugin = core.data.features.plugins.plugins.find((p) => p.id === parsed?.plugin);
  const def = plugin?.manifest.contributes?.attachmentPreviews?.find((d) => d.id === parsed?.name);
  if (!plugin || !parsed || !def)
    return errorResponse("not_found", `no renderer ${request.renderer}`);
  const attachment = core.data.bodies.get(request.message_id)?.attachments[request.index];
  if (!attachment) return errorResponse("not_found", `no attachment ${String(request.index)}`);
  if (!running(core, plugin)) return errorResponse("unavailable", `${plugin.id} is not running`);
  if (!granted(plugin, "attachments", accountOfMessage(core, request.message_id))) {
    return errorResponse("forbidden", `${plugin.id} may not read attachments of this account`);
  }
  if (
    !matchesFile(def.mimeTypes ?? [], def.extensions ?? [], attachment.mime, attachment.filename)
  ) {
    return errorResponse(
      "invalid_request",
      `${request.renderer} does not render ${attachment.mime}`,
    );
  }
  if (attachment.size > Math.min(def.maxBytes ?? MAX_PREVIEW_INPUT, MAX_PREVIEW_INPUT)) {
    return errorResponse("too_large", `${attachment.filename ?? "attachment"} is too large`);
  }
  const render =
    core.data.features.plugins.extensions.behaviours[plugin.id]?.previews?.[parsed.name];
  if (!render) return errorResponse("unavailable", `${plugin.id} rendered nothing`);
  activityOf(core, plugin.id).cpuMs += 20;
  return {
    type: "AttachmentPreview",
    message_id: request.message_id,
    index: request.index,
    renderer: request.renderer,
    preview: render(attachment, request.message_id),
  };
}

/** The contributions an action id may name, by target kind (and their permission). */
function actionDef(core: MockCore, plugin: PluginInfo, name: string, target: PluginActionTarget) {
  const c = plugin.manifest.contributes;
  switch (target.kind) {
    case "composer":
      return granted(plugin, "composer")
        ? c?.composerActions?.find((a) => a.id === name)
        : undefined;
    case "attachment":
      return granted(plugin, "attachments", accountOfMessage(core, target.message_id))
        ? c?.attachmentActions?.find((a) => a.id === name)
        : undefined;
    case "link":
      return c?.linkActions?.find((a) => a.id === name);
  }
}

function runAction(
  core: MockCore,
  request: Extract<PluginExtensionRequest, { type: "RunPluginAction" }>,
): Response {
  const parsed = parseExtensionId(request.action);
  const state = core.data.features.plugins;
  const plugin = state.plugins.find((p) => p.id === parsed?.plugin);
  if (!plugin || !parsed || !actionDef(core, plugin, parsed.name, request.target)) {
    return errorResponse("not_found", `no action ${request.action}`);
  }
  if (!running(core, plugin)) return errorResponse("unavailable", `${plugin.id} is not running`);
  const handler = state.extensions.behaviours[plugin.id]?.actions?.[parsed.name];
  if (handler && state.host) {
    const api = pluginApi(core, state.host, plugin);
    const target = structuredClone(request.target);
    // The plug-in takes the action; what it does arrives as events.
    core.later(() => {
      handler(api, target, request.client_id, core);
    }, 0);
  }
  activityOf(core, plugin.id).cpuMs += 10;
  return { type: "Done" };
}

export function answerPluginExtensions(core: MockCore, request: PluginExtensionRequest): Response {
  const state = core.data.features.plugins;
  if (!state.host && request.type !== "GetPluginActivity" && request.type !== "OverrideSendVeto") {
    return errorResponse("unsupported", `${request.type} needs the plug-in host`);
  }
  switch (request.type) {
    case "GetPluginActivity": {
      const plugin = state.plugins.find((p) => p.id === request.plugin_id);
      if (!plugin) return errorResponse("not_found", `plug-in not found: ${request.plugin_id}`);
      return { type: "PluginActivity", activity: activityResponse(core, plugin) };
    }
    case "GetMessagePluginAnnotations":
      return annotations(core, request.message_id);
    case "RunPluginJob": {
      const plugin = state.plugins.find((p) => p.id === request.plugin_id);
      if (!plugin) return errorResponse("not_found", `plug-in not found: ${request.plugin_id}`);
      return runJob(core, plugin, request.job);
    }
    case "RenderAttachmentPreview":
      return preview(core, request);
    case "RunPluginAction":
      return runAction(core, request);
    case "OverrideSendVeto": {
      const veto = state.extensions.vetoes.get(request.veto_id);
      state.extensions.vetoes.delete(request.veto_id);
      if (!veto || veto.until < Date.now()) {
        return errorResponse("not_found", `no send veto ${request.veto_id} is pending`);
      }
      return sendDraft(core, veto.draftId, veto.delaySecs, veto.sendAt, veto.crypto);
    }
  }
}

/** Test / demo helper: `<plugin>.<name>` of a contribution. */
export { extensionId };
