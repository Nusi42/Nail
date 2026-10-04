/**
 * The Markdown preview: the HTML the core would send (`RenderPreview`:
 * rendered, sanitized, the identity's mail stylesheet inlined), shown in a
 * sandboxed iframe like received mail.
 */
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { IdentityId } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { setSrcdoc } from "../../theme/trustedHtml";

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
  // `srcdoc` is a Trusted Types sink (enforced by the remote UI).
  const frame = useRef<HTMLIFrameElement>(null);
  const html = preview.data ?? "";
  useLayoutEffect(() => {
    if (frame.current) setSrcdoc(frame.current, html);
  }, [html, preview.isError]);
  return (
    <div data-part="composer.preview" data-state={preview.isError ? "error" : undefined}>
      {preview.isError ? (
        <div data-part="composer.preview.error" role="alert">
          {t("composer-preview-failed")}
        </div>
      ) : (
        <iframe
          ref={frame}
          data-part="composer.preview.frame"
          sandbox=""
          referrerPolicy="no-referrer"
          title={t("composer-preview-title")}
        />
      )}
    </div>
  );
}
