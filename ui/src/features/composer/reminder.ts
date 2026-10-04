/**
 * The attachment reminder: the message says "attached" (in the user's
 * language, from `composer-attachment-keywords`) but has no attachment.
 * Quoted text and the signature do not count.
 */
import type { ComposerMode } from "../../api/generated";

/** The localized keyword list (comma separated in the catalog). */
export function parseKeywords(list: string): string[] {
  return list
    .split(",")
    .map((k) => k.trim().toLocaleLowerCase())
    .filter((k) => k !== "");
}

/** Text the user wrote: without quoted lines, quoted HTML and the signature. */
export function ownText(mode: ComposerMode, body: string): string {
  if (mode === "html") {
    const doc = new DOMParser().parseFromString(body, "text/html");
    doc.querySelectorAll("blockquote, .signature, .forwarded").forEach((el) => {
      el.remove();
    });
    return doc.body.textContent;
  }
  const lines: string[] = [];
  for (const line of body.split("\n")) {
    if (line === "-- ") break;
    if (!line.startsWith(">")) lines.push(line);
  }
  return lines.join("\n");
}

/** Whether `text` contains one of the keywords as a whole word. */
export function mentionsAttachment(text: string, keywords: readonly string[]): boolean {
  const words = text.toLocaleLowerCase().split(/[^\p{L}\p{N}]+/u);
  const set = new Set(words);
  return keywords.some((k) =>
    k.includes(" ") ? text.toLocaleLowerCase().includes(k) : set.has(k),
  );
}

/** Whether the reminder applies to this message. */
export function needsReminder(
  mode: ComposerMode,
  body: string,
  subject: string,
  attachments: number,
  keywords: readonly string[],
): boolean {
  if (attachments > 0) return false;
  return mentionsAttachment(`${subject}\n${ownText(mode, body)}`, keywords);
}
