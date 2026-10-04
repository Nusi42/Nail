import { chars } from "../../text/chars";
/**
 * Calendar colours (WP164, ADR 0109): the palette the calendar list's "…"
 * menu and the calendar dialogs offer, which colour values the UI accepts
 * from data, and how an event card shows its calendar's colour.
 *
 * A card is the theme's background tinted with a little of the calendar
 * colour (`EVENT_TINT` per cent) plus a solid bar in the colour on the
 * leading edge, and its text is always the theme's `fg`. A light tint keeps
 * the card's luminance close to the theme's background, so the text stays
 * readable for any colour in every theme (`colors.test.ts` checks the
 * palette, black and white against all five built-in themes). High
 * contrast drops the tint (`skin.ts`).
 *
 * Colours are data: they reach the stylesheet only through geometry rules
 * (`useGeometry`), never inline styles, and only when they look like a CSS
 * colour (`#rgb`, `#rrggbb`, `#rrggbbaa` or a colour keyword).
 */

/** Per cent of the calendar colour in an event card's background. */
export const EVENT_TINT = 16;

/** Ids of the palette's colours (`pim-color-<id>` names each). */
export type CalendarColorId =
  "blue" | "cyan" | "green" | "amber" | "orange" | "red" | "pink" | "purple" | "slate";

/** One colour of the palette: id and value. */
export interface PaletteColor {
  id: CalendarColorId;
  value: string;
}

/** The colours offered for calendars and task lists. */
export const CALENDAR_COLORS: readonly PaletteColor[] = [
  { id: "blue", value: "#2563eb" },
  { id: "cyan", value: "#0891b2" },
  { id: "green", value: "#16a34a" },
  { id: "amber", value: "#ca8a04" },
  { id: "orange", value: "#ea580c" },
  { id: "red", value: "#dc2626" },
  { id: "pink", value: "#db2777" },
  { id: "purple", value: "#9333ea" },
  { id: "slate", value: "#64748b" },
];

const HEX_RE = /^#(?:[0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/i;
const KEYWORD_RE = /^[a-z]{3,32}$/i;

/** Whether `value` may be used as a colour in the stylesheet. */
export function isCssColor(value: string | null | undefined): value is string {
  if (!value) return false;
  const v = value.trim();
  return HEX_RE.test(v) || KEYWORD_RE.test(v);
}

/** The palette entry of a value (case-insensitive), if it is one. */
export function paletteColorOf(value: string | null | undefined): PaletteColor | null {
  const v = (value ?? "").trim().toLowerCase();
  return CALENDAR_COLORS.find((c) => c.value === v) ?? null;
}

/**
 * Geometry declarations of an event card (time grid event, chip, agenda
 * row) in `color`; `{}` (the theme's look) without a usable colour.
 */
export function eventCardDeclarations(color: string | null): Record<string, string> {
  if (!isCssColor(color)) return {};
  const c = color.trim();
  return {
    "border-left-color": c,
    "background-color": `color-mix(in srgb, ${c} ${String(EVENT_TINT)}%, var(--nail-color-bg))`,
  };
}

/** Geometry declarations of a colour square (calendar list, palette). */
export function swatchDeclarations(color: string | null): Record<string, string> {
  if (!isCssColor(color)) return {};
  const c = color.trim();
  return { "background-color": c, "border-color": c };
}

// ---------------------------------------------------------------------------
// Colour math (WCAG 2.x), for the contrast checks of the event cards.

/** `#rgb` / `#rrggbb` → [r, g, b] (0–255); `null` for anything else. */
export function parseHex(hex: string): [number, number, number] | null {
  const h = hex.trim().replace(/^#/, "");
  const full =
    h.length === 3
      ? chars(h)
          .map((c) => c + c)
          .join("")
      : h.length === 6 || h.length === 8
        ? h.slice(0, 6)
        : null;
  if (full === null || !/^[0-9a-f]{6}$/i.test(full)) return null;
  return [0, 2, 4].map((i) => parseInt(full.slice(i, i + 2), 16)) as [number, number, number];
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

/** WCAG relative luminance of an sRGB colour (0–255 channels). */
export function luminance([r, g, b]: readonly [number, number, number]): number {
  const channel = (v: number) => {
    const c = v / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

/** WCAG contrast ratio of two colours (1–21). */
export function contrastRatio(
  a: readonly [number, number, number],
  b: readonly [number, number, number],
): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}
