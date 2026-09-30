/**
 * Plug-in commands as layout actions (ADR 0050): `layout.kdl` refers to a
 * command as `plugin.<plugin>.<command>` in toolbars, menus, context menus
 * and key bindings, exactly like a built-in action.
 *
 * The action registry is static, so the plug-in actions are made known
 * through the layout's action metadata: `withPluginActions` adds, for every
 * command of every installed plug-in, an action definition with the
 * command's title and icon (unless `layout.kdl` defines its own label /
 * icon) that invokes `plugins.run-command` with the command's id. The
 * dispatcher follows that alias to the handler below, which asks the core
 * to run the command (`RunPluginCommand`) with this UI's context and id.
 * Commands of plug-ins that are not installed stay without a handler:
 * their buttons render disabled and their keys do nothing.
 */
import { useMemo } from "react";
import type { QueryClient } from "@tanstack/react-query";
import type { ActionHandler } from "../../actions/registry";
import type {
  ActionDef,
  PluginCommandDef,
  PluginCommandSurface,
  PluginInfo,
  ResolvedLayout,
} from "../../api/generated";
import type { UiState } from "../../state/uiStore";
import { clientIdOf, surfaceOfKeyContext, uiContextOf } from "./context";
import { parsePluginCommandId, pluginCommandId } from "./ids";
import { pluginKeys, usePlugins } from "./queries";

/** The action every plug-in command is an alias of (argument: the command id). */
export const RUN_COMMAND = "plugins.run-command";

/** Whether a plug-in's runtime can take calls. */
export function isRunnable(plugin: PluginInfo): boolean {
  return plugin.enabled && !["disabled", "crashed", "incompatible"].includes(plugin.state);
}

/** The command `action` names among `plugins`, with its plug-in. */
export function findCommand(
  plugins: readonly PluginInfo[] | undefined,
  action: string,
): { plugin: PluginInfo; command: PluginCommandDef } | null {
  const parsed = parsePluginCommandId(action);
  if (!parsed || !plugins) return null;
  const plugin = plugins.find((p) => p.id === parsed.plugin);
  const command = plugin?.manifest.contributes?.commands?.find((c) => c.id === parsed.command);
  return plugin && command ? { plugin, command } : null;
}

type ContextState = Pick<UiState, "keyContext" | "selectedMessageId" | "composer">;

function satisfied(surface: PluginCommandSurface, state: ContextState): boolean {
  switch (surface) {
    case "global":
      return true;
    case "list":
    case "reader":
      return state.selectedMessageId !== null;
    case "composer":
      return state.composer !== null;
  }
}

/**
 * The surface a command runs on now, or `null` when none of its contexts
 * applies (a list / reader command needs a selected message, a composer
 * command an open composer; no contexts = everywhere).
 */
export function surfaceFor(
  command: PluginCommandDef,
  state: ContextState,
): PluginCommandSurface | null {
  const current = surfaceOfKeyContext(state.keyContext);
  const contexts = command.contexts ?? [];
  if (contexts.length === 0) return current;
  if (contexts.includes(current) && satisfied(current, state)) return current;
  return contexts.find((c) => satisfied(c, state)) ?? null;
}

function pluginsIn(queryClient: QueryClient): PluginInfo[] | undefined {
  return queryClient.getQueryData<PluginInfo[]>(pluginKeys.list);
}

/** The handler of `plugins.run-command "<plugin.x.y>"`. */
export const runCommandHandler: ActionHandler = {
  run: async (ctx, args) => {
    const action = args[0];
    if (typeof action !== "string") return;
    const found = findCommand(pluginsIn(ctx.queryClient), action);
    if (!found || !isRunnable(found.plugin)) return;
    const state = ctx.store.getState();
    const surface = surfaceFor(found.command, state);
    if (surface === null) return;
    await ctx.client.request(
      {
        type: "RunPluginCommand",
        command: action,
        surface,
        context: uiContextOf(state),
        client_id: clientIdOf(ctx.client),
      },
      "Done",
    );
  },
  enabled: (ctx, args) => {
    const action = args[0];
    if (typeof action !== "string") return false;
    const found = findCommand(pluginsIn(ctx.queryClient), action);
    return (
      found !== null &&
      isRunnable(found.plugin) &&
      surfaceFor(found.command, ctx.store.getState()) !== null
    );
  },
};

/**
 * The layout with an action definition for every command of `plugins`
 * (see the module comment). The user's own label / icon win; an action the
 * user made an alias of something else is left alone. Returns `layout`
 * itself when nothing changes.
 */
export function withPluginActions(
  layout: ResolvedLayout,
  plugins: readonly PluginInfo[] | undefined,
): ResolvedLayout {
  if (!plugins || plugins.length === 0) return layout;
  const added: Record<string, ActionDef> = {};
  for (const plugin of plugins) {
    for (const command of plugin.manifest.contributes?.commands ?? []) {
      const id = pluginCommandId(plugin.id, command.id);
      const own = Object.hasOwn(layout.actions, id) ? layout.actions[id] : undefined;
      if (own?.alias_of) continue;
      added[id] = {
        label: own?.label ?? command.title,
        icon: own?.icon ?? command.icon ?? null,
        alias_of: RUN_COMMAND,
        args: [id],
      };
    }
  }
  if (Object.keys(added).length === 0) return layout;
  return { ...layout, actions: { ...layout.actions, ...added } };
}

/**
 * `layout` with the plug-in commands (for `App.tsx` / the test harness to
 * wrap the resolved layout with before it reaches the providers).
 */
export function usePluginLayout(layout: ResolvedLayout): ResolvedLayout {
  const plugins = usePlugins();
  return useMemo(() => withPluginActions(layout, plugins.data), [layout, plugins.data]);
}
