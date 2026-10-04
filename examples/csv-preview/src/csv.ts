/**
 * CSV / TSV parsing (RFC 4180: quoted fields, doubled quotes, line breaks
 * inside quotes, CRLF or LF) with delimiter detection. No Nail API calls
 * here, so the logic is easy to test.
 */

/** Delimiters the parser recognizes. */
export const DELIMITERS = [",", ";", "\t", "|"] as const;
/** A delimiter. */
export type Delimiter = (typeof DELIMITERS)[number];

/** Longest cell text kept for the preview (characters). */
export const MAX_CELL = 500;
/** Most columns the preview shows. */
export const MAX_COLUMNS = 50;

/**
 * Picks the delimiter: tab for `.tsv` files and the TSV type, else the
 * one that occurs most often outside quotes in the first line (comma when
 * none does).
 */
export function detectDelimiter(text: string, name = "", mimeType = ""): Delimiter {
  if (/\.tsv$/i.test(name) || mimeType.toLowerCase() === "text/tab-separated-values") return "\t";
  const counts = new Map<Delimiter, number>(DELIMITERS.map((d) => [d, 0]));
  let quoted = false;
  for (const ch of text) {
    if (ch === '"') quoted = !quoted;
    else if (!quoted && (ch === "\n" || ch === "\r")) break;
    else if (!quoted && counts.has(ch as Delimiter)) {
      counts.set(ch as Delimiter, (counts.get(ch as Delimiter) ?? 0) + 1);
    }
  }
  let best: Delimiter = ",";
  for (const d of DELIMITERS) if ((counts.get(d) ?? 0) > (counts.get(best) ?? 0)) best = d;
  return best;
}

/**
 * Parses CSV text into rows of fields. Stops after `maxRows` rows and says
 * whether there were more. A trailing line break does not make an empty
 * row; a leading byte order mark is dropped.
 */
export function parseCsv(
  text: string,
  delimiter: Delimiter,
  maxRows = Infinity,
): { rows: string[][]; more: boolean; total: number } {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let quoted = false;
  let total = 0;
  let i = text.charCodeAt(0) === 0xfeff ? 1 : 0;
  const endRow = () => {
    row.push(field);
    field = "";
    total += 1;
    if (rows.length < maxRows) rows.push(row);
    row = [];
  };
  for (; i < text.length; i += 1) {
    const ch = text[i];
    if (quoted) {
      if (ch === '"') {
        if (text[i + 1] === '"') {
          field += '"';
          i += 1;
        } else {
          quoted = false;
        }
      } else {
        field += ch ?? "";
      }
    } else if (ch === '"' && field === "") {
      quoted = true;
    } else if (ch === delimiter) {
      row.push(field);
      field = "";
    } else if (ch === "\r" || ch === "\n") {
      if (ch === "\r" && text[i + 1] === "\n") i += 1;
      endRow();
    } else {
      field += ch ?? "";
    }
  }
  if (field !== "" || row.length > 0) endRow();
  return { rows, more: total > rows.length, total };
}

/** A table for the preview: column labels and rows of the same length. */
export interface Table {
  /** Column labels. */
  columns: string[];
  /** Rows, each as long as `columns`. */
  rows: string[][];
  /** All data rows of the file (without the header). */
  total: number;
}

/**
 * Shapes parsed rows into a table: the first row as the header (when
 * asked), at most {@link MAX_COLUMNS} columns, short rows padded, long
 * cells cut. `columnLabel(n)` names a column without a header (1-based).
 */
export function toTable(
  parsed: { rows: string[][]; total: number },
  headerRow: boolean,
  maxRows: number,
  columnLabel: (number: number) => string,
): Table {
  const all = parsed.rows;
  const header = headerRow ? (all[0] ?? []) : [];
  const data = (headerRow ? all.slice(1) : all).slice(0, maxRows);
  const width = Math.min(MAX_COLUMNS, Math.max(header.length, ...data.map((r) => r.length), 0));
  const cut = (s: string) => (s.length > MAX_CELL ? `${s.slice(0, MAX_CELL - 1)}…` : s);
  const columns = Array.from({ length: width }, (_, i) => {
    const label = (header[i] ?? "").trim();
    return label ? cut(label) : columnLabel(i + 1);
  });
  const rows = data.map((r) => Array.from({ length: width }, (_, i) => cut(r[i] ?? "")));
  return {
    columns,
    rows,
    total: Math.max(0, parsed.total - (headerRow && all.length > 0 ? 1 : 0)),
  };
}
