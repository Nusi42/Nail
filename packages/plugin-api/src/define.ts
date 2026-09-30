/**
 * `definePlugin` — declares a plug-in (the default export of its bundle).
 */
import type {
  NailCommandsApi,
  NailSettingsApi,
  PluginActivationContext,
  PluginDefinition,
  PluginIdentity,
} from "./api";
import { host, toPanelProvider, type PanelProvider, type PanelRender } from "./nail";

/** The context `activate` receives: the host's, with JSX panels. */
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
}

/** What a plug-in author writes. */
export interface PluginSetup {
  /** Called once when the plug-in starts: register commands and panels. */
  activate(ctx: PluginContext): void | Promise<void>;
  /** Called when the plug-in stops; synchronous (the host does not wait). */
  deactivate?(): void;
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
      };
      return setup.activate(ctx);
    },
  };
  if (setup.deactivate) {
    const deactivate = setup.deactivate;
    definition.deactivate = () => deactivate();
  }
  return definition;
}
