/**
 * Finding an attachment mention: keyword lists (from the plug-in's
 * catalogs, so each language has its own words, plus the user's own
 * words from the settings) matched against the subject and the text the
 * user wrote — not the signature, and not the quoted message unless the
 * user asks for it. No Nail API calls here, so the logic is easy to test.
 */

/** A keyword: a word or phrase, or a prefix (`attach*`). */
export interface Keyword {
  /** The words, as written (without the `*`). */
  text: string;
  /** Whether longer words starting with it match too. */
  prefix: boolean;
}

/**
 * Parses a keyword list: entries separated by commas or new lines, a
 * trailing `*` for "every word starting with this". Empty entries and
 * duplicates (ignoring case) are dropped.
 */
export function parseKeywords(list: string): Keyword[] {
  const seen = new Set<string>();
  const out: Keyword[] = [];
  for (const raw of list.split(/[,\n]/)) {
    const entry = raw.trim().replace(/\s+/g, " ");
    const prefix = entry.endsWith("*");
    const text = (prefix ? entry.slice(0, -1) : entry).trim();
    if (!text || text.includes("*")) continue;
    const key = `${text.toLowerCase()}${prefix ? "*" : ""}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push({ text, prefix });
  }
  return out;
}

/** The text to check: what the user wrote, without signature and (unless asked) quotes. */
export function ownText(text: string, checkQuoted: boolean): string {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const signature = lines.indexOf("-- ");
  const body = signature >= 0 ? lines.slice(0, signature) : lines;
  return (checkQuoted ? body : body.filter((line) => !/^\s*>/.test(line))).join("\n");
}

const WORD = "[\\p{L}\\p{N}]";

function escape(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** A pattern for a keyword: whole words, any case, any whitespace between words. */
function pattern(keyword: Keyword): RegExp {
  const words = escape(keyword.text).replace(/ /g, "\\s+");
  const end = keyword.prefix ? `${WORD}*` : `(?!${WORD})`;
  return new RegExp(`(^|[^\\p{L}\\p{N}])(${words}${end})`, "iu");
}

/**
 * The first keyword mention in `text` (as written there), or `null`.
 */
export function findMention(text: string, keywords: Keyword[]): string | null {
  let best: { at: number; word: string } | null = null;
  for (const keyword of keywords) {
    const match = pattern(keyword).exec(text);
    if (!match) continue;
    const at = match.index + (match[1] ?? "").length;
    if (!best || at < best.at) best = { at, word: match[2] ?? "" };
  }
  return best ? best.word : null;
}
