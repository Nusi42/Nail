/**
 * Fuzzy search of the command palette (pure): every typed word must match
 * the entry's localized title or one of its keywords — as a word start
 * (best), anywhere inside, or as letters in order ("rpa" → "Reply to
 * all"). Accents and case are ignored. Matches in the title are returned
 * as ranges for highlighting. Unavailable entries always come after the
 * available ones; recent entries get a small boost.
 */
import type { PaletteEntry } from "../../actions/palette";

/** A half-open range `[start, end)` in the title. */
export type Range = readonly [number, number];

export interface ScoredEntry {
  entry: PaletteEntry;
  score: number;
  /** Matched parts of the title. */
  ranges: Range[];
}

/** Lowercase without accents, one output character per input character. */
export function fold(text: string): string {
  let out = "";
  for (const ch of text) {
    const base = ch.normalize("NFD").replace(/\p{M}+/gu, "");
    const lower = (base === "" ? ch : base).toLowerCase();
    // Keep the 1:1 mapping (`ß` → `ss` would shift the ranges).
    out += lower.length === ch.length ? lower : ch.toLowerCase().slice(0, ch.length);
  }
  return out;
}

function isWordStart(text: string, index: number): boolean {
  if (index === 0) return true;
  const before = text[index - 1] ?? " ";
  return !/[\p{L}\p{N}]/u.test(before);
}

interface TokenMatch {
  score: number;
  ranges: Range[];
}

/** The best way `token` (folded) matches `text` (folded). */
export function matchToken(token: string, text: string): TokenMatch | null {
  if (token === "") return { score: 0, ranges: [] };
  // Whole substring, preferably at a word start.
  let best: TokenMatch | null = null;
  let from = 0;
  for (;;) {
    const at = text.indexOf(token, from);
    if (at < 0) break;
    const start = isWordStart(text, at);
    const end = at + token.length;
    const whole = start && (end === text.length || !/[\p{L}\p{N}]/u.test(text[end] ?? ""));
    const score = (start ? 100 : 60) + (whole ? 20 : 0) - Math.min(at, 30) * 0.5;
    if (!best || score > best.score) best = { score, ranges: [[at, end]] };
    from = at + 1;
  }
  if (best) return best;
  // Letters in order; word starts and runs of letters count more. Jumping
  // to word starts can miss a later letter: then take the plain order.
  return lettersInOrder(token, text, true) ?? lettersInOrder(token, text, false);
}

function lettersInOrder(token: string, text: string, preferStarts: boolean): TokenMatch | null {
  const ranges: [number, number][] = [];
  let score = 20;
  let index = 0;
  let previous = -2;
  for (const ch of token) {
    const next = text.indexOf(ch, index);
    if (next < 0) return null;
    let found = next;
    // Keep a run going; else prefer the next word start with this letter.
    if (preferStarts && next !== previous + 1) {
      for (let i = next; i < text.length; i++) {
        if (text[i] === ch && isWordStart(text, i)) {
          found = i;
          break;
        }
      }
    }
    if (found === previous + 1) {
      score += 6;
      const last = ranges[ranges.length - 1];
      if (last) last[1] = found + 1;
    } else {
      score += isWordStart(text, found) ? 8 : 1;
      score -= Math.min(found - index, 10) * 0.3;
      ranges.push([found, found + 1]);
    }
    previous = found;
    index = found + 1;
  }
  return { score, ranges };
}

/** Merges overlapping or touching ranges. */
export function mergeRanges(ranges: readonly Range[]): Range[] {
  const sorted = [...ranges].sort((a, b) => a[0] - b[0]);
  const out: [number, number][] = [];
  for (const [s, e] of sorted) {
    const last = out[out.length - 1];
    if (last && s <= last[1]) last[1] = Math.max(last[1], e);
    else out.push([s, e]);
  }
  return out;
}

/** The words typed into the palette. */
export function queryTokens(query: string): string[] {
  return fold(query)
    .split(/\s+/)
    .filter((t) => t.length > 0);
}

/** Scores one entry; `null` when a word does not match. */
export function scoreEntry(entry: PaletteEntry, tokens: readonly string[]): ScoredEntry | null {
  const title = fold(entry.title);
  const keywords = entry.keywords.map(fold);
  let score = 0;
  const ranges: Range[] = [];
  for (const token of tokens) {
    const inTitle = matchToken(token, title);
    let bestKeyword: TokenMatch | null = null;
    for (const keyword of keywords) {
      const m = matchToken(token, keyword);
      if (m && (!bestKeyword || m.score > bestKeyword.score)) bestKeyword = m;
    }
    const keywordScore = bestKeyword ? bestKeyword.score * 0.8 : -1;
    if (inTitle && inTitle.score >= keywordScore) {
      score += inTitle.score;
      ranges.push(...inTitle.ranges);
    } else if (bestKeyword) {
      score += keywordScore;
    } else {
      return null;
    }
  }
  if (entry.recentRank !== null) score += 15 / (entry.recentRank + 1);
  return { entry, score, ranges: mergeRanges(ranges) };
}

/**
 * The palette's rows for `query`: without a query the entries in their
 * order (recent first) with unavailable ones last; with a query the
 * matches by score.
 */
export function searchPalette(entries: readonly PaletteEntry[], query: string): ScoredEntry[] {
  const tokens = queryTokens(query);
  const scored =
    tokens.length === 0
      ? entries.map((entry) => ({ entry, score: 0, ranges: [] as Range[] }))
      : entries.map((e) => scoreEntry(e, tokens)).filter((s): s is ScoredEntry => s !== null);
  const order = new Map(entries.map((e, i) => [e, i]));
  return scored.sort((a, b) => {
    if (a.entry.available !== b.entry.available) return a.entry.available ? -1 : 1;
    if (tokens.length > 0 && b.score !== a.score) return b.score - a.score;
    return (order.get(a.entry) ?? 0) - (order.get(b.entry) ?? 0);
  });
}
