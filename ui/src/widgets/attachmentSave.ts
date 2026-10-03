/**
 * Saving an attachment of received mail (M5a WP155). The desktop app asks
 * where with the system's Save dialog (`PickPath`) and the core writes the
 * file (`SaveAttachment`; the dialog already asked before replacing a
 * file). Without the desktop shell (`nail serve`, the remote UI) the core
 * answers `PickPath` with `unsupported`; the browser then downloads the
 * decoded content (`GetMessageAttachment`).
 */
import type { NailClient } from "../api/client";
import { NailError } from "../api/client";
import type { AttachmentInfo, MessageId, Response } from "../api/generated";

/** How a save ended. */
export type SaveOutcome = "saved" | "downloaded" | "cancelled" | "unsupported";

/** Error codes of a core that does not have the request. */
const UNSUPPORTED = new Set(["unsupported", "invalid_request"]);

function fail(response: Extract<Response, { type: "Error" }>): never {
  throw new NailError(response.code, response.message, response.localized ?? null);
}

/** Hands `data` (base64) to the browser as a file download. */
export function downloadFile(name: string, mime: string, data: string): boolean {
  if (typeof URL.createObjectURL !== "function") return false;
  const bytes = Uint8Array.from(atob(data), (c) => c.charCodeAt(0));
  const url = URL.createObjectURL(new Blob([bytes], { type: mime }));
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.rel = "noopener";
  document.body.append(link);
  link.click();
  link.remove();
  // After the click has started the download.
  setTimeout(() => URL.revokeObjectURL(url), 0);
  return true;
}

export async function saveAttachment(
  client: NailClient,
  messageId: MessageId,
  attachment: AttachmentInfo,
  name: string,
  download: (name: string, mime: string, data: string) => boolean = downloadFile,
): Promise<SaveOutcome> {
  const picked = await client.call({
    type: "PickPath",
    pick: {
      kind: "save-file",
      title_id: "message-view-attachment-save-title",
      filters: [],
      default_path: name,
    },
  });
  if (picked.type === "PathPicked") {
    if (picked.path === null) return "cancelled";
    const saved = await client.call({
      type: "SaveAttachment",
      message_id: messageId,
      index: attachment.index,
      path: picked.path,
      // The Save dialog asked before replacing a file.
      overwrite: true,
    });
    if (saved.type === "Error") {
      if (UNSUPPORTED.has(saved.code)) return "unsupported";
      fail(saved);
    }
    return "saved";
  }
  if (picked.type === "Error" && picked.code !== "unsupported") fail(picked);
  // No desktop shell: download through the browser.
  const content = await client.call({
    type: "GetMessageAttachment",
    message_id: messageId,
    index: attachment.index,
  });
  if (content.type === "Error") {
    if (UNSUPPORTED.has(content.code)) return "unsupported";
    fail(content);
  }
  if (content.type !== "AttachmentData") return "unsupported";
  return download(name, content.mime, content.data) ? "downloaded" : "unsupported";
}
