/**
 * Attachment previews drawn by plug-ins (M4a, ADR 0079). A renderer
 * answers a declarative tree (drawn like a panel, read-only) or a raster
 * image (PNG / JPEG / GIF / WebP as a `data:` URI) — never HTML, never
 * SVG. The pane always names the plug-in that drew it.
 *
 * Mount point: `AttachmentPreviewPane` in the reader where an attachment
 * is previewed (it renders nothing when no running plug-in renders that
 * type); `previewRenderersFor` tells whether to offer a preview at all.
 */
import { useState } from "react";
import { errorMessageOf } from "../../api/client";
import type { AttachmentInfo, MessageId, PluginPreview } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { PREVIEW_IMAGE_TYPES, previewRenderersFor, type Offer } from "./extensions";
import { partAttrs } from "./parts";
import { useAttachmentPreview, usePlugins } from "./queries";
import { isSafeImage, PluginTree } from "./tree/PluginTree";

/** The `data:` URI of an image preview, or `null` when it is not an allowed raster image. */
export function previewImageSrc(preview: Extract<PluginPreview, { kind: "image" }>): string | null {
  if (!PREVIEW_IMAGE_TYPES.includes(preview.mimeType)) return null;
  if (!/^[A-Za-z0-9+/]*={0,2}$/.test(preview.data)) return null;
  const src = `data:${preview.mimeType};base64,${preview.data}`;
  return isSafeImage(src) ? src : null;
}

const noop = () => undefined;

function PreviewBody({ renderer, preview }: { renderer: Offer; preview: PluginPreview }) {
  const t = useT();
  if (preview.kind === "tree") {
    return (
      <PluginTree
        plugin={renderer.pluginId}
        nodes={preview.tree}
        values={{}}
        issues={{}}
        onChange={noop}
        onAction={noop}
        busy
      />
    );
  }
  const src = previewImageSrc(preview);
  if (src === null) {
    return (
      <p data-part="plugin-preview.error" role="alert">
        {t("plugin-preview-invalid", { plugin: renderer.pluginName })}
      </p>
    );
  }
  return (
    <img
      {...partAttrs("plugin.image", renderer.pluginId)}
      data-preview=""
      src={src}
      alt={preview.alt ?? t("plugin-preview-image-alt", { plugin: renderer.pluginName })}
    />
  );
}

/** Whether a plug-in can preview this attachment (the reader offers "Preview" then). */
export function useHasAttachmentPreview(
  attachment: AttachmentInfo,
  accountId: string | null = null,
): boolean {
  const plugins = usePlugins();
  return previewRenderersFor(plugins.data ?? [], attachment, accountId).length > 0;
}

/** A plug-in's preview of one attachment, with a choice of renderer when several apply. */
export function AttachmentPreviewPane({
  messageId,
  attachment,
  accountId = null,
  id,
}: {
  messageId: MessageId;
  attachment: AttachmentInfo;
  accountId?: string | null;
  /** Element id, so the attachment card can move focus here (M5a). */
  id?: string;
}) {
  const t = useT();
  const plugins = usePlugins();
  const renderers = previewRenderersFor(plugins.data ?? [], attachment, accountId);
  const [chosen, setChosen] = useState<string | null>(null);
  const renderer = renderers.find((r) => r.id === chosen) ?? renderers[0] ?? null;
  const preview = useAttachmentPreview(messageId, attachment.index, renderer?.id ?? null);
  if (renderer === null) return null;
  const name = attachment.filename ?? attachment.mime;
  return (
    <section
      id={id}
      tabIndex={id === undefined ? undefined : -1}
      data-part="plugin-preview"
      data-plugin={renderer.pluginId}
      data-renderer={renderer.id}
      aria-label={t("plugin-preview-label", { name })}
      aria-busy={preview.isFetching}
    >
      <header data-part="plugin-preview.header">
        <span data-part="plugin-preview.by">
          {t("plugin-preview-by", { renderer: renderer.title, plugin: renderer.pluginName })}
        </span>
        {renderers.length > 1 && (
          <label data-part="plugin-preview.choice">
            {t("plugin-preview-choose")}
            <select
              data-part="plugin-preview.select"
              value={renderer.id}
              onChange={(event) => setChosen(event.target.value)}
            >
              {renderers.map((r) => (
                <option key={r.id} value={r.id}>
                  {t("plugin-preview-option", { renderer: r.title, plugin: r.pluginName })}
                </option>
              ))}
            </select>
          </label>
        )}
      </header>
      {preview.isPending && (
        <p data-part="plugin-preview.loading" role="status">
          {t("plugin-preview-loading")}
        </p>
      )}
      {preview.isError && (
        <p data-part="plugin-preview.error" role="alert">
          {t(errorMessageOf(preview.error).id, errorMessageOf(preview.error).args)}
        </p>
      )}
      {preview.data && (
        <div data-part="plugin-preview.body">
          <PreviewBody renderer={renderer} preview={preview.data} />
        </div>
      )}
    </section>
  );
}
