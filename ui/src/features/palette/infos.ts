/**
 * The action descriptions the command palette lists (built-ins, features,
 * plug-in commands), shared with the toolbar customizer (M5d WP182) so it
 * need not load the palette dialog's module (which `index.ts` loads
 * lazily). Imports the app registry: only lazily loaded code imports this.
 */
import type { ActionInfo } from "../../actions/palette";
import type { useLayoutConfig } from "../../layout/LayoutContext";
import { RUN_COMMAND } from "../plugins/commands";
import { appRegistry } from "../registry";
import { BUILTIN_INFO } from "./builtinInfo";

/** Every action description: built-ins, then the features' own (which win). */
export function paletteInfos(): Record<string, ActionInfo> {
  return { ...BUILTIN_INFO, ...appRegistry.actionInfo };
}

/** Plug-in commands in the layout (`plugin.<id>.<command>` → title). */
export function pluginTitles(layout: ReturnType<typeof useLayoutConfig>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [id, def] of Object.entries(layout.actions)) {
    if (def.alias_of === RUN_COMMAND && def.label !== null && !def.label.startsWith("@")) {
      out[id] = def.label;
    }
  }
  return out;
}
