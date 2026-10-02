/**
 * "CSV preview" — an attachment preview renderer: CSV and TSV attachments
 * are shown as a table in Nail's preview pane. The renderer gets the
 * attachment's bytes and answers a component tree (a `<table>`); Nail
 * draws it in its own style — previews are never HTML.
 *
 * The text is read as UTF-8, or as Windows-1252 when it is not valid
 * UTF-8 (many spreadsheet exports). The delimiter (comma, semicolon, tab,
 * bar) is detected from the first line.
 *
 * Permissions (manifest.json): attachments — the renderer sees only the
 * attachment the user previews, and only if it is not end-to-end
 * encrypted.
 */
import { decodeText, definePlugin, nail, type PluginAttachmentRef } from "@nail/plugin-api";

import { detectDelimiter, parseCsv, toTable } from "./csv";

const t = (id: string, args?: Record<string, string | number>) => nail.l10n.t(id, args);

/** The setting `max-rows`, within the limits of a table component. */
function maxRows(): number {
  const value = nail.settings.get("max-rows");
  return typeof value === "number" && value >= 1 ? Math.min(Math.floor(value), 499) : 200;
}

/** Renders the preview of one attachment. */
export function renderTable(attachment: PluginAttachmentRef, bytes: Uint8Array) {
  const text = decodeText(bytes);
  const delimiter = detectDelimiter(text, attachment.name, attachment.mimeType);
  const headerRow = nail.settings.get("header-row") !== false;
  const limit = maxRows();
  const parsed = parseCsv(text, delimiter, limit + 1);
  const table = toTable(parsed, headerRow, limit, (number) => t("column", { number }));
  if (table.columns.length === 0) {
    return <text style="muted">{t("empty")}</text>;
  }
  return (
    <stack>
      <text style="muted">
        {t("summary", { rows: table.total, columns: table.columns.length })}
      </text>
      <table columns={table.columns.map((label) => ({ label }))} rows={table.rows} />
      {table.total > table.rows.length && (
        <text style="muted">{t("truncated", { shown: table.rows.length })}</text>
      )}
    </stack>
  );
}

export default definePlugin({
  attachments: { previews: { table: renderTable } },
});
