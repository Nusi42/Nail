/**
 * Plug-in ids in the layout (ADR 0050), the same rules as
 * `nail_api::plugins`: plug-in ids and names of commands / panels are
 * kebab-case, 1–64 characters, starting with a letter; commands are the
 * actions `plugin.<plugin>.<command>`, panels `plugin-panel
 * "<plugin>.<panel>"`.
 */

/** Longest plug-in id or contribution name (`MAX_NAME_LEN`). */
export const MAX_NAME_LEN = 64;

/** Prefix of plug-in action ids. */
export const PLUGIN_ACTION_PREFIX = "plugin.";

const KEBAB = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/;

/** Whether `name` is a valid plug-in id or contribution name. */
export function isPluginName(name: string): boolean {
  return name.length > 0 && name.length <= MAX_NAME_LEN && KEBAB.test(name);
}

/** `plugin.<plugin>.<command>`. */
export function pluginCommandId(plugin: string, command: string): string {
  return `${PLUGIN_ACTION_PREFIX}${plugin}.${command}`;
}

/** Splits `plugin.<plugin>.<command>`; `null` for anything else. */
export function parsePluginCommandId(action: string): { plugin: string; command: string } | null {
  if (!action.startsWith(PLUGIN_ACTION_PREFIX)) return null;
  const rest = action.slice(PLUGIN_ACTION_PREFIX.length);
  const dot = rest.indexOf(".");
  if (dot < 0) return null;
  const plugin = rest.slice(0, dot);
  const command = rest.slice(dot + 1);
  return isPluginName(plugin) && isPluginName(command) ? { plugin, command } : null;
}

/** Splits `<plugin>.<panel>`; `null` for anything else. */
export function parsePluginPanelId(id: string): { plugin: string; panel: string } | null {
  const dot = id.indexOf(".");
  if (dot < 0) return null;
  const plugin = id.slice(0, dot);
  const panel = id.slice(dot + 1);
  return isPluginName(plugin) && isPluginName(panel) ? { plugin, panel } : null;
}
