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

import { hexContrastRatio } from "../../theme/contrast";

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
      const ratio = hexContrastRatio(fgValue, bgValue);
      if (ratio !== null && ratio < TEXT_CONTRAST) {
        out.push({ fg: `color.${fg}`, bg: `color.${bg}`, ratio, needed: TEXT_CONTRAST });
      }
    }
  }
  return out.sort((a, b) => a.ratio - b.ratio);
}
