/**
 * sRGB colour math of the WCAG 2.x contrast checks, defined once for the UI
 * (wave Q2, SSOT / ODR / DRY): hex parsing, relative luminance, contrast
 * ratios and `color-mix` in sRGB. The single source is `nail_util::color`
 * (nail-config's theme contrast test uses it); `parseHex` and
 * `hexContrastRatio` are pinned by nail-api's `appearance` vectors.
 */

/** Red, green and blue, 0–255 (fractions after mixing). */
export type Rgb = readonly [number, number, number];

/** `#rgb` / `#rrggbb` (any case, surrounding spaces ignored) → its channels; anything else → `null`. */
export function parseHex(value: string): [number, number, number] | null {
  const v = value.trim().toLowerCase();
  const short = /^#([0-9a-f])([0-9a-f])([0-9a-f])$/.exec(v);
  const long = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/.exec(v);
  const parts = short ? short.slice(1).map((c) => c + c) : long?.slice(1);
  if (!parts) return null;
  const [r = "0", g = "0", b = "0"] = parts;
  return [parseInt(r, 16), parseInt(g, 16), parseInt(b, 16)];
}

/** WCAG 2.x relative luminance (0 = black, 1 = white), sRGB threshold 0.04045. */
export function luminance([r, g, b]: Rgb): number {
  const channel = (c: number) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

/** The WCAG contrast ratio of two colours (1–21; order does not matter). */
export function contrastRatio(a: Rgb, b: Rgb): number {
  const [la, lb] = [luminance(a), luminance(b)];
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

/** The contrast ratio of two hex colours, `null` when one is not hex ({@link parseHex}). */
export function hexContrastRatio(a: string, b: string): number | null {
  const ca = parseHex(a);
  const cb = parseHex(b);
  return ca && cb ? contrastRatio(ca, cb) : null;
}

/** `color-mix(in srgb, a p%, b)` of two hex colours. */
export function mixHex(a: string, percent: number, b: string): [number, number, number] {
  const x = parseHex(a);
  const y = parseHex(b);
  if (!x || !y) throw new Error(`not a hex colour: ${x ? b : a}`);
  const p = percent / 100;
  const mix = (u: number, v: number) => u * p + v * (1 - p);
  return [mix(x[0], y[0]), mix(x[1], y[1]), mix(x[2], y[2])];
}

/** The readable text colour on a background: black on light colours, white on dark ones. */
export function textOn(background: Rgb): "#000000" | "#ffffff" {
  // Black wins above ~0.179 (equal contrast with both at 0.179).
  return luminance(background) > 0.179 ? "#000000" : "#ffffff";
}
