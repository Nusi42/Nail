/**
 * Saving a file the core makes (M5a WP155: attachments of received mail;
 * ADR 0190: connection log exports). The desktop app asks where with the
 * system's Save dialog (`PickPath`) and the core writes the file (the
 * dialog already asked before replacing a file). Without the desktop shell
 * (`nail serve`, the remote UI) the core answers `PickPath` with
 * `unsupported`; the browser then downloads the content instead.
 */
import type { NailClient } from "../api/client";
import { NailError } from "../api/client";
import type { AttachmentInfo, MessageId, PathFilter, Response } from "../api/generated";
import { UNSUPPORTED_CODES } from "../api/contract";

/** How a save ended. */
export type SaveOutcome = "saved" | "downloaded" | "cancelled" | "unsupported";

function fail(response: Extract<Response, { type: "Error" }>): never {
  throw new NailError(response.code, response.message, response.localized ?? null);
}

/** Hands a blob to the browser as a file download. */
function downloadBlob(name: string, blob: Blob): boolean {
  if (typeof URL.createObjectURL !== "function") return false;
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.rel = "noopener";
  document.body.append(link);
  link.click();
  link.remove();
  // After the click has started the download.
  setTimeout(() => {
    URL.revokeObjectURL(url);
  }, 0);
  return true;
}

/** Hands `data` (base64) to the browser as a file download. */
export function downloadFile(name: string, mime: string, data: string): boolean {
  const bytes = Uint8Array.from(atob(data), (c) => c.charCodeAt(0));
  return downloadBlob(name, new Blob([bytes], { type: mime }));
}

/** Hands `text` to the browser as a UTF-8 text file download. */
export function downloadText(name: string, text: string): boolean {
  return downloadBlob(name, new Blob([text], { type: "text/plain;charset=utf-8" }));
}

/** What the Save dialog asks (`PathPick` of kind `save-file`). */
export interface SavePick {
  /** Fluent id of the dialog title. */
  titleId: string;
  /** Suggested file name. */
  name: string;
  /** File-type filters. */
  filters?: PathFilter[];
}

/**
 * The shared save flow: `PickPath` → `saveTo(path)` on the desktop; without
 * the desktop shell `download()` (whether the browser took the file; `null`
 * when the core has nothing to download).
 */
export async function saveThroughPicker(
  client: NailClient,
  pick: SavePick,
  saveTo: (path: string) => Promise<Response>,
  download: () => Promise<boolean | null>,
): Promise<SaveOutcome> {
  const picked = await client.call({
    type: "PickPath",
    pick: {
      kind: "save-file",
      title_id: pick.titleId,
      filters: pick.filters ?? [],
      default_path: pick.name,
    },
  });
  if (picked.type === "PathPicked") {
    if (picked.path === null) return "cancelled";
    const saved = await saveTo(picked.path);
    if (saved.type === "Error") {
      if (UNSUPPORTED_CODES.has(saved.code)) return "unsupported";
      fail(saved);
    }
    return "saved";
  }
  if (picked.type === "Error" && picked.code !== "unsupported") fail(picked);
  // No desktop shell: download through the browser.
  return (await download()) === true ? "downloaded" : "unsupported";
}

/** Whether `response` is an error of a core without the request. */
export function isUnsupported(response: Response): boolean {
  return response.type === "Error" && UNSUPPORTED_CODES.has(response.code);
}

export async function saveAttachment(
  client: NailClient,
  messageId: MessageId,
  attachment: AttachmentInfo,
  name: string,
  download: (name: string, mime: string, data: string) => boolean = downloadFile,
): Promise<SaveOutcome> {
  return saveThroughPicker(
    client,
    { titleId: "message-view-attachment-save-title", name },
    (path) =>
      client.call({
        type: "SaveAttachment",
        message_id: messageId,
        index: attachment.index,
        path,
        // The Save dialog asked before replacing a file.
        overwrite: true,
      }),
    async () => {
      const content = await client.call({
        type: "GetMessageAttachment",
        message_id: messageId,
        index: attachment.index,
      });
      if (isUnsupported(content)) return null;
      if (content.type === "Error") fail(content);
      if (content.type !== "AttachmentData") return null;
      return download(name, content.mime, content.data);
    },
  );
}
