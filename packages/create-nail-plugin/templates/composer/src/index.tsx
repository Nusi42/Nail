/**
 * A starter composer plug-in (see manifest.json):
 *
 * - "Insert today's date" inserts the date at the cursor;
 * - "UPPER CASE" replaces the selected text with capital letters;
 * - a Markdown block (```` ```__PLUGIN_ID__.callout ````) rendered as a
 *   highlighted note in the HTML part, and as text in the plain-text part.
 *
 * Composer edits apply to the draft the action ran on, live; the user can
 * undo them like typing. The HTML a block renders is sanitized by Nail
 * like any message body.
 */
import { definePlugin, nail } from "@nail/plugin-api";

const pad = (n: number) => String(n).padStart(2, "0");

/** A local date as `YYYY-MM-DD`. */
export function isoDate(date: Date): string {
  return `${String(date.getFullYear())}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** Escapes text for HTML. */
export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

export default definePlugin({
  composer: {
    actions: {
      "insert-date": async (ctx) => {
        await nail.composer.insert(isoDate(new Date()), { draftId: ctx.composer?.draftId });
      },
      "upper-case": async (ctx) => {
        const selected = ctx.composer?.selection.text ?? "";
        if (!selected) return;
        await nail.composer.replaceSelection(selected.toUpperCase(), {
          draftId: ctx.composer?.draftId,
        });
      },
    },
    markdownBlocks: {
      callout: ({ source }) => {
        const label = nail.l10n.t("callout-label");
        const text = source.trim();
        return {
          html: `<blockquote><p><strong>${escapeHtml(label)}:</strong> ${escapeHtml(text)}</p></blockquote>`,
          text: `${label}: ${text}`,
        };
      },
    },
  },
});
