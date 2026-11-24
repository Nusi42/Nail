/**
 * The Markdown preview: the HTML the core would send (`RenderPreview`:
 * rendered, sanitized, the identity's mail stylesheet inlined), shown in a
 * sandboxed iframe like received mail.
 *
 * R8: the frame grows with its content like a mail frame (the frame script
 * reports the height, `framedDocument`; opaque origin, nothing remote); the
 * skin keeps it between the editor's minimum and `PREVIEW_MAX_HEIGHT`, and
 * a longer preview scrolls inside it.
 */
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import type { IdentityId } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { MAIL_FRAME_SANDBOX } from "../../theme/frameHeight";
import { framedDocument } from "../../theme/reader";
import { useGeometry } from "../../theme/ThemeProvider";
import { useLocalFrame } from "../../widgets/MailFrame";

/** Typing pause before the preview is rendered again. */
export const PREVIEW_DELAY_MS = 300;

export function Preview({
  markdown,
  identityId,
}: {
  markdown: string;
  identityId: IdentityId | null;
}) {
  const t = useT();
  const client = useClient();
  const [source, setSource] = useState(markdown);
  useEffect(() => {
    const timer = setTimeout(() => {
      setSource(markdown);
    }, PREVIEW_DELAY_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [markdown]);
  const preview = useQuery({
    queryKey: ["composer", "preview", identityId, source],
    queryFn: async () =>
      (
        await client.request(
          { type: "RenderPreview", markdown: source, identity_id: identityId },
          "PreviewRendered",
        )
      ).html,
    placeholderData: keepPreviousData,
    retry: false,
  });
  return (
    <div data-part="composer.preview" data-state={preview.isError ? "error" : undefined}>
      {preview.isError ? (
        <div data-part="composer.preview.error" role="alert">
          {t("composer-preview-failed")}
        </div>
      ) : (
        <PreviewFrame html={preview.data ?? ""} title={t("composer-preview-title")} />
      )}
    </div>
  );
}

/** Links in the preview go nowhere (it is the message being written). */
const ignoreLink = () => undefined;

/** The preview's frame, as tall as its content (within the skin's limits). */
function PreviewFrame({ html, title }: { html: string; title: string }) {
  const frame = useRef<HTMLIFrameElement>(null);
  const document = useMemo(() => framedDocument(html), [html]);
  // `srcdoc` is a Trusted Types sink (enforced by the remote UI).
  const height = useLocalFrame(frame, document, ignoreLink);
  const geom = `composer-preview-${useId()}`;
  useGeometry(geom, height === null ? {} : { height: `${String(height)}px` });
  return (
    <iframe
      ref={frame}
      data-part="composer.preview.frame"
      data-state={height === null ? undefined : "measured"}
      data-geom={geom}
      sandbox={MAIL_FRAME_SANDBOX}
      referrerPolicy="no-referrer"
      title={title}
    />
  );
}
