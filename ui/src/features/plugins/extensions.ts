/**
 * Which plug-in extensions apply where (M4a, ADR 0079), mirrored from
 * `nail_api::plugins::extensions` for the UI: preview renderers and
 * actions for an attachment, actions for a link, composer actions and
 * Markdown blocks, the "what it adds" list of a manifest, write
 * permissions, and applying a composer edit to plain text.
 *
 * Only running plug-ins with the needed grant contribute (the core checks
 * again; this only decides what the UI offers).
 */
import type {
  AttachmentInfo,
  ComposerEdit,
  ComposerSelection,
  PluginHookKind,
  PluginInfo,
  PluginManifest,
  PluginPermission,
} from "../../api/generated";

/** Largest attachment a preview renderer gets (`MAX_PREVIEW_INPUT`). */
export const MAX_PREVIEW_INPUT = 10 * 1024 * 1024;
/** Image types a preview may be (`PREVIEW_IMAGE_TYPES`; never SVG). */
export const PREVIEW_IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/** Permissions that let a plug-in change the user's data (`PluginPermission::is_write`). */
export const WRITE_PERMISSIONS: readonly PluginPermission[] = [
  "mail.organize",
  "mail.send",
  "calendar.write",
  "contacts.write",
  "tasks.write",
];

export function isWritePermission(permission: PluginPermission): boolean {
  return WRITE_PERMISSIONS.includes(permission);
}

/** Whether the plug-in runs (enabled, started, not crashed). */
export function isRunning(plugin: PluginInfo): boolean {
  return plugin.enabled && plugin.state === "running";
}

/** Whether the plug-in holds a grant of `permission` (for `accountId`, when given). */
export function hasGrant(
  plugin: PluginInfo,
  permission: PluginPermission,
  accountId?: string | null,
): boolean {
  const grant = plugin.grants.find((g) => g.permission === permission);
  if (!grant) return false;
  return grant.accounts === null || accountId == null || grant.accounts.includes(accountId);
}

/** The id of an extension across plug-ins: `<plugin>.<name>` (`plugin_extension_id`). */
export function extensionId(pluginId: string, name: string): string {
  return `${pluginId}.${name}`;
}

/** Splits an extension id (`parse_plugin_extension_id`). */
export function parseExtensionId(id: string): { plugin: string; name: string } | null {
  const dot = id.indexOf(".");
  if (dot <= 0 || dot === id.length - 1) return null;
  return { plugin: id.slice(0, dot), name: id.slice(dot + 1) };
}

/** `matches_file`: a MIME pattern (`type/*` allowed) or the file's extension. */
export function matchesFile(
  mimeTypes: readonly string[],
  extensions: readonly string[],
  mime: string,
  filename: string | null,
): boolean {
  const type = mime.toLowerCase();
  const byType = mimeTypes.some((pattern) =>
    pattern.endsWith("/*") ? type.split("/")[0] === pattern.slice(0, -2) : pattern === type,
  );
  const dot = filename?.lastIndexOf(".") ?? -1;
  const ext = filename && dot >= 0 ? filename.slice(dot + 1).toLowerCase() : null;
  return byType || (ext !== null && extensions.includes(ext));
}

/** An extension offered in a menu or pane. */
export interface Offer {
  /** `<plugin>.<name>`. */
  id: string;
  pluginId: string;
  pluginName: string;
  /** The title (resolved by the core). */
  title: string;
  icon: string | null;
  description: string | null;
}

function offer(
  plugin: PluginInfo,
  def: { id: string; title: string; icon?: string; description?: string },
): Offer {
  return {
    id: extensionId(plugin.id, def.id),
    pluginId: plugin.id,
    pluginName: plugin.manifest.name,
    title: def.title,
    icon: def.icon ?? null,
    description: def.description ?? null,
  };
}

/** The running plug-ins allowed to see attachments (of `accountId`). */
function attachmentPlugins(plugins: readonly PluginInfo[], accountId?: string | null) {
  return plugins.filter((p) => isRunning(p) && hasGrant(p, "attachments", accountId));
}

/** Preview renderers for an attachment (permission `attachments`, size limit). */
export function previewRenderersFor(
  plugins: readonly PluginInfo[],
  attachment: AttachmentInfo,
  accountId?: string | null,
): Offer[] {
  return attachmentPlugins(plugins, accountId).flatMap((p) =>
    (p.manifest.contributes?.attachmentPreviews ?? [])
      .filter(
        (def) =>
          matchesFile(
            def.mimeTypes ?? [],
            def.extensions ?? [],
            attachment.mime,
            attachment.filename,
          ) && attachment.size <= Math.min(def.maxBytes ?? MAX_PREVIEW_INPUT, MAX_PREVIEW_INPUT),
      )
      .map((def) => offer(p, def)),
  );
}

/** Actions in an attachment's menu (permission `attachments`). */
export function attachmentActionsFor(
  plugins: readonly PluginInfo[],
  attachment: AttachmentInfo,
  accountId?: string | null,
): Offer[] {
  return attachmentPlugins(plugins, accountId).flatMap((p) =>
    (p.manifest.contributes?.attachmentActions ?? [])
      .filter((def) => {
        const mimes = def.mimeTypes ?? [];
        const exts = def.extensions ?? [];
        return (
          (mimes.length === 0 && exts.length === 0) ||
          matchesFile(mimes, exts, attachment.mime, attachment.filename)
        );
      })
      .map((def) => offer(p, def)),
  );
}

/** The host a link action is matched against: the URL's host, empty for `mailto:`. */
export function linkHost(url: string): string | null {
  try {
    const parsed = new URL(url);
    if (parsed.protocol === "mailto:") return "";
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
    return parsed.hostname.toLowerCase();
  } catch {
    return null;
  }
}

/** `PluginPermission::allows_host` for a host pattern (`*.example.com`). */
export function hostMatches(pattern: string, host: string): boolean {
  const h = host.replace(/\.+$/, "").toLowerCase();
  if (pattern.startsWith("*.")) {
    const parent = pattern.slice(2);
    if (!h.endsWith(parent)) return false;
    const sub = h.slice(0, h.length - parent.length);
    return sub.length > 1 && sub.endsWith(".");
  }
  return h === pattern;
}

/** Actions in a link's menu (no permission: they get only the link). */
export function linkActionsFor(plugins: readonly PluginInfo[], url: string): Offer[] {
  const host = linkHost(url);
  if (host === null) return [];
  return plugins.filter(isRunning).flatMap((p) =>
    (p.manifest.contributes?.linkActions ?? [])
      .filter((def) => {
        const hosts = def.hosts ?? [];
        return hosts.length === 0 || hosts.some((pattern) => hostMatches(pattern, host));
      })
      .map((def) => offer(p, def)),
  );
}

/** Composer actions (permission `composer`). */
export function composerActionsFor(plugins: readonly PluginInfo[]): Offer[] {
  return plugins
    .filter((p) => isRunning(p) && hasGrant(p, "composer"))
    .flatMap((p) => (p.manifest.contributes?.composerActions ?? []).map((def) => offer(p, def)));
}

/** A Markdown block a plug-in renders at send time. */
export interface BlockOffer extends Offer {
  /** Text inserted as the block's body when picked from the menu. */
  template: string;
}

/** Markdown blocks (permission `composer`). */
export function markdownBlocksFor(plugins: readonly PluginInfo[]): BlockOffer[] {
  return plugins
    .filter((p) => isRunning(p) && hasGrant(p, "composer"))
    .flatMap((p) =>
      (p.manifest.contributes?.markdownBlocks ?? []).map((def) => ({
        ...offer(p, def),
        template: def.template ?? "",
      })),
    );
}

/** The plug-in block a fence info string (`acme-crm.ticket`) names, if one renders it. */
export function markdownBlockOwner(
  plugins: readonly PluginInfo[],
  info: string,
): BlockOffer | null {
  return markdownBlocksFor(plugins).find((b) => b.id === info.trim()) ?? null;
}

/** The Markdown text that inserts a plug-in block (a fenced block with its info string). */
export function blockFence(block: Pick<BlockOffer, "id" | "template">): string {
  const body =
    block.template.endsWith("\n") || block.template === "" ? block.template : `${block.template}\n`;
  const longest = Math.max(2, ...[...body.matchAll(/`+/g)].map((m) => m[0].length));
  const fence = "`".repeat(longest + 1);
  return `${fence}${block.id}\n${body}${fence}\n`;
}

/** Fluent id of a hook's name (`plugin-hook-*` in core-plugins.ftl). */
export function hookMessageId(hook: PluginHookKind): string {
  return hook === "beforeSend" ? "plugin-hook-before-send" : `plugin-hook-${hook}`;
}

/** One line of "what it adds". */
export interface ContributionItem {
  kind:
    | "command"
    | "panel"
    | "hook"
    | "composerAction"
    | "markdownBlock"
    | "attachmentPreview"
    | "attachmentAction"
    | "linkAction"
    | "job"
    | "mcpTool";
  key: string;
  /** Fluent id (`contributionMessageId`) and arguments; `hook` is a hook message id. */
  id: string;
  args: Record<string, string>;
}

/** Fluent id of a contribution line (`plugin-install-composer-action`, …). */
export function contributionMessageId(kind: ContributionItem["kind"]): string {
  return `plugin-install-${kind.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}`;
}

/** Everything a manifest contributes, in a stable order. */
export function contributionItems(manifest: PluginManifest): ContributionItem[] {
  const c = manifest.contributes;
  const item = (
    kind: ContributionItem["kind"],
    key: string,
    args: Record<string, string>,
  ): ContributionItem => ({ kind, key: `${kind}:${key}`, id: contributionMessageId(kind), args });
  return [
    ...(c?.commands ?? []).map((x) => item("command", x.id, { title: x.title })),
    ...(c?.panels ?? []).map((x) => item("panel", x.id, { title: x.title })),
    ...(c?.hooks ?? []).map((x) => item("hook", x.hook, { hook: hookMessageId(x.hook) })),
    ...(c?.composerActions ?? []).map((x) => item("composerAction", x.id, { title: x.title })),
    ...(c?.markdownBlocks ?? []).map((x) => item("markdownBlock", x.id, { title: x.title })),
    ...(c?.attachmentPreviews ?? []).map((x) =>
      item("attachmentPreview", x.id, { title: x.title }),
    ),
    ...(c?.attachmentActions ?? []).map((x) => item("attachmentAction", x.id, { title: x.title })),
    ...(c?.linkActions ?? []).map((x) => item("linkAction", x.id, { title: x.title })),
    ...(c?.jobs ?? []).map((x) => item("job", x.id, { title: x.title ?? x.id })),
    ...(c?.mcpTools ?? []).map((x) => item("mcpTool", x.id, { tool: `${manifest.id}__${x.id}` })),
  ];
}

/**
 * A composer edit applied to plain text (`ComposerEdit`): insert at the
 * cursor (replacing nothing), replace the selection, or replace all.
 * Returns the new text and the cursor after the inserted text.
 */
export function applyComposerEdit(
  source: string,
  selection: Pick<ComposerSelection, "start" | "end">,
  edit: Pick<ComposerEdit, "op" | "text">,
): { text: string; cursor: number } {
  const clamp = (n: number) => Math.max(0, Math.min(source.length, n));
  const start = clamp(Math.min(selection.start, selection.end));
  const end = clamp(Math.max(selection.start, selection.end));
  switch (edit.op) {
    case "replace-all":
      return { text: edit.text, cursor: edit.text.length };
    case "insert":
      return {
        text: source.slice(0, end) + edit.text + source.slice(end),
        cursor: end + edit.text.length,
      };
    case "replace-selection":
      return {
        text: source.slice(0, start) + edit.text + source.slice(end),
        cursor: start + edit.text.length,
      };
  }
}
