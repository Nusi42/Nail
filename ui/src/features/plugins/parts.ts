/**
 * Theme parts of plug-in UI (ADR 0050, "every element themeable").
 *
 * Every element a plug-in tree produces carries a generic part
 * (`data-part="plugin.button"`) and the plug-in's id (`data-plugin=
 * "follow-up"`); input nodes also carry their id (`data-id="due"`). A theme
 * styles all plug-ins through the generic parts, one plug-in with a
 * condition (`part "plugin.button[plugin=\"follow-up\"]"`), and — once the
 * theme compiler maps them (`pluginPartSelector`) — with the namespaced
 * names of the design, `plugin.<id>.<part>` (`plugin.follow-up.button`).
 */
import { noticeStack } from "../../theme/declarations";

/** The generic parts plug-in elements carry. */
export const PLUGIN_PARTS = [
  "plugin.tree",
  "plugin.stack",
  "plugin.text",
  "plugin.markdown",
  "plugin.markdown.heading",
  "plugin.markdown.paragraph",
  "plugin.markdown.list",
  "plugin.markdown.item",
  "plugin.markdown.quote",
  "plugin.markdown.code",
  "plugin.markdown.code-block",
  "plugin.markdown.rule",
  "plugin.markdown.link",
  "plugin.markdown.confirm",
  "plugin.markdown.confirm.text",
  "plugin.markdown.confirm.button",
  "plugin.field",
  "plugin.field.label",
  "plugin.field.error",
  "plugin.input",
  "plugin.textarea",
  "plugin.select",
  "plugin.checkbox",
  "plugin.date",
  "plugin.contact-picker",
  "plugin.contact-picker.list",
  "plugin.contact-picker.contact",
  "plugin.contact-picker.remove",
  "plugin.contact-picker.input",
  "plugin.folder-picker",
  "plugin.message-list",
  "plugin.message-list.row",
  "plugin.message-list.choice",
  "plugin.message-list.subject",
  "plugin.message-list.from",
  "plugin.message-list.date",
  "plugin.table",
  "plugin.table.header",
  "plugin.table.row",
  "plugin.table.cell",
  "plugin.table.choice",
  "plugin.button",
  "plugin.button.icon",
  "plugin.button.label",
  "plugin.progress",
  "plugin.progress.label",
  "plugin.image",
  "plugin.dialog",
  "plugin.dialog.header",
  "plugin.dialog.plugin",
  "plugin.dialog.body",
  "plugin.dialog.error",
  "plugin.dialog.buttons",
  "plugin.dialog.button",
  "plugin.panel",
  "plugin.panel.error",
  "plugin.notifications",
  "plugin.notifications.list",
  "plugin.notification",
  "plugin.notification.content",
  "plugin.notification.plugin",
  "plugin.notification.title",
  "plugin.notification.body",
  "plugin.notification.dismiss",
  "plugin.notification.dismiss.icon",
] as const;

export type PluginPart = (typeof PLUGIN_PARTS)[number];

const GENERIC = new Set<string>(PLUGIN_PARTS);

/** Attributes of one plug-in element: its part, plug-in and optional id. */
export function partAttrs(
  part: PluginPart,
  plugin: string,
  id?: string,
): { "data-part": PluginPart; "data-plugin": string; "data-id"?: string } {
  return id === undefined
    ? { "data-part": part, "data-plugin": plugin }
    : { "data-part": part, "data-plugin": plugin, "data-id": id };
}

/**
 * The CSS selector of a namespaced part name for the theme compiler:
 * `plugin.<id>.<part>` → `[data-part="plugin.<part>"][data-plugin="<id>"]`.
 * `null` when `part` is not such a name (generic parts are left alone).
 */
export function pluginPartSelector(part: string): string | null {
  if (GENERIC.has(part) || !part.startsWith("plugin.")) return null;
  const rest = part.slice("plugin.".length);
  const dot = rest.indexOf(".");
  if (dot <= 0) return null;
  const plugin = rest.slice(0, dot);
  const generic = `plugin.${rest.slice(dot + 1)}`;
  if (!GENERIC.has(generic) || !/^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/.test(plugin)) return null;
  return `[data-part="${generic}"][data-plugin="${plugin}"]`;
}

/**
 * Structural CSS of the plug-in UI for the theme compiler's base rules
 * (`BASE_CSS` in `src/theme/compile.ts`; themes may override any of it):
 * where notifications sit (dialogs are windows of the window manager, M5c), stack directions, and
 * lists without bullets. Colours, spacing and fonts come from `theme.kdl`.
 */
export const PLUGIN_BASE_CSS = `[data-part="plugin.dialog"] { display: flex; flex-direction: column; min-height: 0; overflow: hidden; }
[data-part="plugin.dialog.header"] { display: flex; align-items: center; flex: 0 0 auto; }
[data-part="plugin.dialog.body"] { display: flex; flex-direction: column; flex: 1 1 auto; min-height: 0; overflow: auto; }
[data-part="plugin.dialog.buttons"] { display: flex; justify-content: flex-end; flex-wrap: wrap; }
[data-part="plugin.stack"] { display: flex; flex-direction: column; min-width: 0; }
[data-part="plugin.stack"][data-direction="horizontal"] { flex-direction: row; flex-wrap: wrap; align-items: center; }
[data-part="plugin.field"] { display: flex; flex-direction: column; }
[data-part="plugin.text"], [data-part="plugin.markdown.code-block"], [data-part="plugin-placements.text"] { white-space: pre-wrap; overflow-wrap: anywhere; }
[data-part="plugin.image"] { max-width: 100%; height: auto; }
[data-part="plugin.notifications"] { ${noticeStack("right")} z-index: 300; }
[data-part="plugin.notification"] { display: flex; align-items: flex-start; }
[data-part="plugin.notification.content"] { display: flex; flex-direction: column; flex: 1 1 auto; min-width: 0; }
[data-part="plugin-panel"] { display: flex; flex-direction: column; min-height: 0; overflow: auto; }
[data-part="plugin-veto"] { display: flex; flex-direction: column; min-height: 0; overflow: auto; }
[data-part="plugin-veto.buttons"], [data-part="plugin-banner.buttons"], [data-part="plugin-composer"], [data-part="plugin-preview.header"] { display: flex; flex-wrap: wrap; justify-content: flex-end; align-items: center; }
[data-part="plugin-veto.reason"], [data-part="plugin-banner.text"], [data-part="plugin-block.source"] { white-space: pre-wrap; overflow-wrap: anywhere; }
[data-part="plugin-banner"] { display: flex; flex-wrap: wrap; align-items: center; }
[data-part="plugin-badges"], [data-part="plugin-activity.event"] { display: inline-flex; flex-wrap: wrap; }
[data-part="plugin-badge"] { display: inline-flex; align-items: center; }
[data-part="plugin-badge.description"] { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
[data-part="plugin-extension-menu"] { position: relative; display: inline-block; }
[data-part="plugin-extension-menu.list"] { position: absolute; z-index: 120; min-width: 100%; list-style: none; margin: 0; padding: 0; }
[data-part="plugin-extension-menu.item"] { display: flex; width: 100%; align-items: center; text-align: start; }
[data-part="plugin-preview.body"] { overflow: auto; max-width: 100%; }
[data-part="plugin-manager.toolbar"] { display: flex; flex-wrap: wrap; align-items: center; }
[data-part="plugin-install.source"] { display: flex; flex-direction: column; gap: var(--nail-space-md); min-width: 0; }
[data-part="plugin-install.kinds"] { display: flex; flex-wrap: wrap; align-items: center; gap: var(--nail-space-xs) var(--nail-space-lg); min-width: 0; margin: 0; padding: var(--nail-space-sm) var(--nail-space-md); border: 1px solid var(--nail-color-border); border-radius: var(--nail-radius-md); }
[data-part="plugin-install.kinds.legend"] { padding: 0 var(--nail-space-xs); }
[data-part="plugin-install.kind"] { display: inline-flex; align-items: center; gap: var(--nail-space-xs); min-height: var(--nail-size-control); }
[data-part="plugin-install.actions"] { display: flex; flex-wrap: wrap; align-items: center; gap: var(--nail-space-sm); }
[data-part="plugin-banners"], [data-part="plugin-badges"], [data-part="plugin-veto.list"], [data-part="plugin-activity.job-list"],
[data-part="plugin.notifications.list"], [data-part="plugin.message-list"], [data-part="plugin.contact-picker.list"], [data-part="plugin-permissions"], [data-part="plugin-permissions.account-list"], [data-part="plugin-manager.list"], [data-part="plugin-placements.items"], [data-part="plugin-placements.conflicts"], [data-part="plugin-logs.list"], [data-part="plugin-install.issue-list"], [data-part="plugin-install.contributions"] { list-style: none; margin: 0; padding: 0; }
`;
