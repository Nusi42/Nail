/**
 * HTML text escaping, defined once for the UI (Q2, SSOT): the composer's
 * plug-in edits and the mock core's Markdown / quoting stand-ins write text
 * into HTML with it.
 */

/** `text` with `&`, `<`, `>` and `"` as entities (safe in element content and quoted attributes). */
export function escapeHtml(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}
