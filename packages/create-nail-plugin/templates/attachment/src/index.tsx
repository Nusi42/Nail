/**
 * A starter attachment plug-in (see manifest.json):
 *
 * - a preview for text files: line and word counts and the first lines,
 *   drawn with Nail's components (previews are component trees or raster
 *   images, never HTML);
 * - an attachment action "Count words" that shows a notification.
 *
 * Handlers get the attachment's bytes as a `Uint8Array`; the sandbox has
 * no `TextDecoder`, so `decodeText` from the SDK turns them into text.
 */
import { decodeText, definePlugin, nail } from "@nail/plugin-api";

/** Lines shown in the preview at most. */
export const PREVIEW_LINES = 40;

/** Line and word counts of a text. */
export function stats(text: string): { lines: string[]; words: number } {
  const lines = text.replace(/\r\n?/g, "\n").replace(/\n$/, "").split("\n");
  const words = text.split(/\s+/).filter(Boolean).length;
  return { lines: text === "" ? [] : lines, words };
}

export default definePlugin({
  attachments: {
    previews: {
      text: (_attachment, bytes) => {
        const { lines, words } = stats(decodeText(bytes));
        const shown = lines.slice(0, PREVIEW_LINES);
        return (
          <stack>
            <text style="strong">
              {nail.l10n.t("preview-summary", { lines: lines.length, words })}
            </text>
            <table
              columns={[
                { label: nail.l10n.t("column-line") },
                { label: nail.l10n.t("column-text") },
              ]}
              rows={shown.map((line, i) => [String(i + 1), line])}
            />
            {lines.length > shown.length && (
              <text style="muted">{nail.l10n.t("preview-more", { shown: shown.length })}</text>
            )}
          </stack>
        );
      },
    },
    actions: {
      "count-words": async (ctx, bytes) => {
        const { words } = stats(decodeText(bytes ?? new Uint8Array()));
        await nail.ui.notify({
          title: nail.l10n.t("words-counted", { name: ctx.attachment?.name ?? "", words }),
        });
      },
    },
  },
});
