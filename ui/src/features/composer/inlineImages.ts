/**
 * Inline images of a reopened HTML draft. The stored HTML references them
 * as `cid:<content id>`; the editor needs something it can show. Images
 * added in this session keep their local preview; for a draft saved
 * earlier (another session, another device) the bytes come from the core:
 * `GetDraftAttachment {draft_id, blob}` → `AttachmentData {mime, data}`
 * (base64), and become a `data:` preview. The stored HTML never changes:
 * `toStoredHtml` maps previews back to `cid:`.
 *
 * The core serves only blobs attached to that draft, with the draft's
 * access rules (ADR 0048); a failure leaves the alt text in the editor.
 */
import type { NailClient } from "../../api/client";
import type { Draft, DraftAttachment } from "../../api/generated";
import type { UiStore } from "../../state/uiStore";
import { referencedCids } from "./draft";
import { richStoreFor } from "./store";

/** The content of one attachment of a saved draft, or `null` when it is not served. */
export async function fetchDraftAttachment(
  client: NailClient,
  draftId: string,
  blob: string,
): Promise<{ mime: string; data: string } | null> {
  const response = await client.call({ type: "GetDraftAttachment", draft_id: draftId, blob });
  return response.type === "AttachmentData" ? { mime: response.mime, data: response.data } : null;
}

/** Inline images of a draft that its HTML references but that have no preview yet. */
export function missingInlineImages(
  draft: Pick<Draft, "html" | "attachments">,
  previews: Readonly<Record<string, string>>,
): DraftAttachment[] {
  const html = draft.html ?? "";
  const referenced = referencedCids(html);
  return draft.attachments.filter(
    (a) =>
      a.content_id !== null &&
      referenced.has(a.content_id.toLowerCase()) &&
      !Object.hasOwn(previews, a.content_id),
  );
}

/** Only images become previews (a `data:` URL of anything else is not shown). */
function isImage(mime: string): boolean {
  return /^image\/[a-z0-9.+-]+$/i.test(mime) && !/svg/i.test(mime);
}

/**
 * Loads previews for the inline images of a reopened draft into the
 * composer's store. Failures are quiet: the editor shows the alt text.
 */
export async function loadInlinePreviews(
  ctx: { client: NailClient; store: UiStore },
  draft: Pick<Draft, "id" | "html" | "attachments">,
): Promise<number> {
  if (draft.id === null || draft.html === null) return 0;
  const rich = richStoreFor(ctx.store);
  const missing = missingInlineImages(draft, rich.getState().inlinePreviews);
  let loaded = 0;
  for (const attachment of missing) {
    const cid = attachment.content_id;
    if (cid === null) continue;
    try {
      const answer = await fetchDraftAttachment(ctx.client, draft.id, attachment.blob);
      if (!answer || !isImage(answer.mime)) continue;
      const url = `data:${answer.mime};base64,${answer.data}`;
      rich.setState((s) => ({ inlinePreviews: { ...s.inlinePreviews, [cid]: url } }));
      loaded++;
    } catch {
      // Not served (older core, offline): the alt text stays.
    }
  }
  return loaded;
}
