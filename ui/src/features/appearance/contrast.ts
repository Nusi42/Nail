/**
 * Contrast checks of the theme editor (M5d review, ADR 0138): an edit that
 * makes the text unreadable (white on white) is still allowed — the user
 * may be on the way to another palette — but the editor warns at once and
 * names the ways back (Ctrl+Z, "Reset appearance" in the command palette).
 * The pairs are the core of `nail-config`'s `tests/theme_contrast.rs`
 * table (WCAG 2.x relative luminance); values that are not plain hex
 * colours (references the resolver could not follow, `color-mix`) are not
 * checked.
 */

/** WCAG AA for body text. */
export const TEXT_CONTRAST = 4.5;

/** A foreground token checked against background tokens. */
interface Pair {
  fg: string;
  bgs: readonly string[];
}

/** Text colour pairs every theme must keep readable (colour token names). */
export const CONTRAST_PAIRS: readonly Pair[] = [
  { fg: "fg", bgs: ["bg", "bg-subtle", "bg-raised", "canvas"] },
  { fg: "fg-muted", bgs: ["bg"] },
  { fg: "accent-fg", bgs: ["accent"] },
  { fg: "selection-fg", bgs: ["selection"] },
];

/** `#abc` / `#aabbcc` → `[r, g, b]` (0–255); anything else → `null`. */
export function parseHex(value: string): [number, number, number] | null {
  const v = value.trim().toLowerCase();
  const short = /^#([0-9a-f])([0-9a-f])([0-9a-f])$/.exec(v);
  const long = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/.exec(v);
  const parts = short ? short.slice(1).map((c) => c + c) : long?.slice(1);
  if (!parts) return null;
  const [r = "0", g = "0", b = "0"] = parts;
  return [parseInt(r, 16), parseInt(g, 16), parseInt(b, 16)];
}

function luminance([r, g, b]: [number, number, number]): number {
  const channel = (c: number) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

/** The WCAG contrast ratio of two hex colours (`null` when one is not hex). */
export function contrastRatio(a: string, b: string): number | null {
  const ca = parseHex(a);
  const cb = parseHex(b);
  if (!ca || !cb) return null;
  const [hi, lo] = [luminance(ca), luminance(cb)].sort((x, y) => y - x);
  return ((hi ?? 0) + 0.05) / ((lo ?? 0) + 0.05);
}

/** A pair below the needed contrast. */
export interface ContrastProblem {
  fg: string;
  bg: string;
  ratio: number;
  needed: number;
}

/**
 * The pairs of `values` (resolved token values by `color.<name>`) below
 * the text contrast, worst first.
 */
export function contrastProblems(values: ReadonlyMap<string, string>): ContrastProblem[] {
  const out: ContrastProblem[] = [];
  for (const { fg, bgs } of CONTRAST_PAIRS) {
    const fgValue = values.get(`color.${fg}`);
    if (fgValue === undefined) continue;
    for (const bg of bgs) {
      const bgValue = values.get(`color.${bg}`);
      if (bgValue === undefined) continue;
      const ratio = contrastRatio(fgValue, bgValue);
      if (ratio !== null && ratio < TEXT_CONTRAST) {
        out.push({ fg: `color.${fg}`, bg: `color.${bg}`, ratio, needed: TEXT_CONTRAST });
      }
    }
  }
  return out.sort((a, b) => a.ratio - b.ratio);
}
