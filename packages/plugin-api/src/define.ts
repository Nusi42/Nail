/**
 * `definePlugin` — declares a plug-in (the default export of its bundle).
 *
 * Besides `activate`, a plug-in can declare its handlers as data:
 *
 * ```tsx
 * export default definePlugin({
 *   commands: { hello: async () => nail.ui.notify("Hello") },
 *   hooks: { beforeSend: (message) => (message.attachments.length ? null : veto("…")) },
 *   attachments: { previews: { csv: (attachment, bytes) => <table … /> } },
 *   jobs: { digest: async (trigger) => { … } },
 * });
 * ```
 *
 * Every handler is registered with the host before `activate` runs, under
 * the name of its manifest entry (`contributes.hooks`, `composerActions`,
 * `markdownBlocks`, `attachmentPreviews`, `attachmentActions`,
 * `linkActions`, `jobs`, `mcpTools`, `commands`, `panels`).
 */
import type {
  NailCommandsApi,
  NailL10nApi,
  NailSettingsApi,
  PluginActionHandler,
  PluginActivationContext,
  PluginArrivalHook,
  PluginBannerActionHandler,
  PluginBeforeSendHook,
  PluginCommandHandler,
  PluginComposerRegistry,
  PluginDefinition,
  PluginHooksRegistry,
  PluginIdentity,
  PluginJobHandler,
  PluginJobsRegistry,
  PluginLinksRegistry,
  PluginMarkdownBlockRenderer,
  PluginMcpRegistry,
  PluginMcpToolHandler,
  PluginOpenHook,
} from "./api";
import type { PluginAttachmentRef, PluginPreview } from "./generated";
import {
  createPluginApiError,
  host,
  toPanelProvider,
  type PanelProvider,
  type PanelRender,
} from "./nail";
import { isPreview, previewTree } from "./extensions";
import type { UiChild } from "./jsx-runtime";

/**
 * A preview renderer in the SDK: it may return a {@link PluginPreview}
 * (`previewTree`, `previewImage`) or JSX / nodes, which become a tree.
 */
export type SdkPreviewRenderer = (
  attachment: PluginAttachmentRef,
  bytes: Uint8Array,
) => PluginPreview | UiChild | Promise<PluginPreview | UiChild>;

/** The SDK's attachment registrations: previews may return JSX. */
export interface SdkAttachmentsRegistry {
  /** Registers a preview renderer (`contributes.attachmentPreviews`). */
  registerPreview(name: string, renderer: SdkPreviewRenderer): void;
  /** Registers an attachment action (`contributes.attachmentActions`). */
  registerAction(name: string, handler: PluginActionHandler): void;
}

/** The context `activate` receives: the host's, with JSX panels and previews. */
export interface PluginContext {
  /** The plug-in's id and version. */
  readonly plugin: PluginIdentity;
  /** Command handlers. */
  readonly commands: NailCommandsApi;
  /** Panel providers; `render` may return JSX. */
  readonly panels: {
    /** Registers the provider (or render function) of a panel of the manifest. */
    register(name: string, panel: PanelProvider | PanelRender): void;
  };
  /** The plug-in's settings. */
  readonly settings: NailSettingsApi;
  /** Mail hooks (`hooks.arrival`, `hooks.beforeSend`, `hooks.open`). */
  readonly hooks: PluginHooksRegistry;
  /** Composer actions and Markdown blocks (`composer`). */
  readonly composer: PluginComposerRegistry;
  /** Attachment previews and actions (`attachments`). */
  readonly attachments: SdkAttachmentsRegistry;
  /** Link actions. */
  readonly links: PluginLinksRegistry;
  /** Background jobs (`background`). */
  readonly jobs: PluginJobsRegistry;
  /** MCP tools (`mcp.expose`). */
  readonly mcp: PluginMcpRegistry;
  /** The plug-in's own translations. */
  readonly l10n: NailL10nApi;
}

/** Mail hooks declared with {@link definePlugin}. */
export interface PluginSetupHooks {
  /** New mail matching the hook's filter (`hooks.arrival`). */
  arrival?: PluginArrivalHook;
  /** Outgoing mail before it is queued (`hooks.beforeSend`). */
  beforeSend?: PluginBeforeSendHook;
  /** A message opened in the reader (`hooks.open`). */
  open?: PluginOpenHook;
  /** Presses on the buttons of banners the open hook answered. */
  onBannerAction?: PluginBannerActionHandler;
}

/** Composer extensions declared with {@link definePlugin}. */
export interface PluginSetupComposer {
  /** Composer actions by name (`contributes.composerActions`). */
  actions?: Record<string, PluginActionHandler>;
  /** Markdown block renderers by name (`contributes.markdownBlocks`). */
  markdownBlocks?: Record<string, PluginMarkdownBlockRenderer>;
}

/** Attachment extensions declared with {@link definePlugin}. */
export interface PluginSetupAttachments {
  /** Preview renderers by name (`contributes.attachmentPreviews`). */
  previews?: Record<string, SdkPreviewRenderer>;
  /** Attachment actions by name (`contributes.attachmentActions`). */
  actions?: Record<string, PluginActionHandler>;
}

/** What a plug-in author writes. */
export interface PluginSetup {
  /** Called once when the plug-in starts, after the handlers below are registered. */
  activate?(ctx: PluginContext): void | Promise<void>;
  /** Called when the plug-in stops; synchronous (the host does not wait). */
  deactivate?(): void;
  /** Command handlers by name (`contributes.commands`). */
  commands?: Record<string, PluginCommandHandler>;
  /** Panels by name (`contributes.panels`). */
  panels?: Record<string, PanelProvider | PanelRender>;
  /** Mail hooks. */
  hooks?: PluginSetupHooks;
  /** Composer actions and Markdown blocks. */
  composer?: PluginSetupComposer;
  /** Attachment previews and actions. */
  attachments?: PluginSetupAttachments;
  /** Link actions by name (`contributes.linkActions`). */
  links?: Record<string, PluginActionHandler>;
  /** Background job handlers by id (`contributes.jobs`). */
  jobs?: Record<string, PluginJobHandler>;
  /** MCP tool handlers by id (`contributes.mcpTools`). */
  mcpTools?: Record<string, PluginMcpToolHandler>;
}

/** Marks a {@link PluginDefinition} made by {@link definePlugin}. */
export const PLUGIN_MARKER = "nail.plugin/1";

/** The object `definePlugin` returns (what the host loads). */
export interface DefinedPlugin extends PluginDefinition {
  /** {@link PLUGIN_MARKER}. */
  readonly kind: typeof PLUGIN_MARKER;
  /** The API version the plug-in was written for. */
  readonly apiVersion: "1";
}

/** A preview renderer's answer as the host wants it. */
export function toPreview(value: PluginPreview | UiChild): PluginPreview {
  return isPreview(value) ? value : previewTree(value as UiChild);
}

/** Wraps an SDK preview renderer for the host (JSX → tree). */
function hostPreview(renderer: SdkPreviewRenderer) {
  return async (attachment: PluginAttachmentRef, bytes: Uint8Array): Promise<PluginPreview> =>
    toPreview(await renderer(attachment, bytes));
}

/**
 * The host's registry (from `ctx`, else the global `nail`), or one that
 * explains the missing host support.
 */
function registry<K extends "hooks" | "composer" | "attachments" | "links" | "jobs" | "mcp">(
  hostCtx: PluginActivationContext | undefined,
  key: K,
): PluginActivationContext[K] {
  const found =
    hostCtx?.[key] ?? (host() as unknown as Partial<Pick<PluginActivationContext, K>>)[key];
  if (found) return found;
  const missing = () => {
    throw createPluginApiError(
      "unsupported",
      `this version of Nail has no ctx.${key}: the plug-in needs a newer Nail`,
    );
  };
  return new Proxy({} as PluginActivationContext[K], { get: () => missing });
}

/**
 * Declares a plug-in:
 *
 * ```tsx
 * export default definePlugin({
 *   activate(ctx) {
 *     ctx.commands.register("hello", async () => {
 *       await nail.ui.notify("Hello from a plug-in");
 *     });
 *   },
 * });
 * ```
 */
export function definePlugin(setup: PluginSetup): DefinedPlugin {
  const definition: DefinedPlugin = {
    kind: PLUGIN_MARKER,
    apiVersion: "1",
    activate(hostCtx?: PluginActivationContext) {
      const api = host();
      const commands = hostCtx?.commands ?? api.commands;
      const attachments = registry(hostCtx, "attachments");
      const ctx: PluginContext = {
        plugin: hostCtx?.plugin ?? api.plugin,
        commands,
        panels: {
          register: (name, panel) => {
            const provider = toPanelProvider(panel);
            if (hostCtx?.panels) hostCtx.panels.register(name, provider);
            else api.ui.registerPanel(name, provider);
          },
        },
        settings: hostCtx?.settings ?? api.settings,
        hooks: registry(hostCtx, "hooks"),
        composer: registry(hostCtx, "composer"),
        attachments: {
          registerPreview: (name, renderer) =>
            attachments.registerPreview(name, hostPreview(renderer)),
          registerAction: (name, handler) => attachments.registerAction(name, handler),
        },
        links: registry(hostCtx, "links"),
        jobs: registry(hostCtx, "jobs"),
        mcp: registry(hostCtx, "mcp"),
        l10n: hostCtx?.l10n ?? api.l10n,
      };
      registerAll(ctx, setup);
      return setup.activate?.(ctx);
    },
  };
  if (setup.deactivate) {
    const deactivate = setup.deactivate;
    definition.deactivate = () => deactivate();
  }
  return definition;
}

/** Registers the handlers a {@link PluginSetup} declares. */
function registerAll(ctx: PluginContext, setup: PluginSetup): void {
  const each = <T>(map: Record<string, T> | undefined, register: (name: string, v: T) => void) => {
    for (const [name, value] of Object.entries(map ?? {})) register(name, value);
  };
  each(setup.commands, (name, handler) => ctx.commands.register(name, handler));
  each(setup.panels, (name, panel) => ctx.panels.register(name, panel));
  const hooks = setup.hooks ?? {};
  if (hooks.arrival) ctx.hooks.register("arrival", hooks.arrival);
  if (hooks.beforeSend) ctx.hooks.register("beforeSend", hooks.beforeSend);
  if (hooks.open) ctx.hooks.register("open", hooks.open);
  if (hooks.onBannerAction) ctx.hooks.onBannerAction(hooks.onBannerAction);
  each(setup.composer?.actions, (name, h) => ctx.composer.registerAction(name, h));
  each(setup.composer?.markdownBlocks, (name, r) => ctx.composer.registerBlock(name, r));
  each(setup.attachments?.previews, (name, r) => ctx.attachments.registerPreview(name, r));
  each(setup.attachments?.actions, (name, h) => ctx.attachments.registerAction(name, h));
  each(setup.links, (name, h) => ctx.links.registerAction(name, h));
  each(setup.jobs, (id, h) => ctx.jobs.register(id, h));
  each(setup.mcpTools, (id, h) => ctx.mcp.registerTool(id, h));
}
