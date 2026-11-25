/**
 * Calendar colours (WP164, ADR 0109): the palette the calendar list's "…"
 * menu and the calendar dialogs offer, which colour values the UI accepts
 * from data, and how an event card shows its calendar's colour.
 *
 * A card is the theme's background tinted with a little of the calendar
 * colour (`EVENT_TINT` per cent; 20 since R7, ADR 0246) plus a solid bar in the colour on the
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
import { CALENDAR_PALETTE } from "../../api/generated";
import type { Calendar, CalendarColor, CalendarId, PaletteColor } from "../../api/generated";
import { linearRgb, parseHex } from "../../theme/contrast";

/** Per cent of the calendar colour in an event card's background. */
export const EVENT_TINT = 20;

/** Ids of the palette's colours (`pim-color-<id>` names each); the core's `CalendarColor`. */
export type CalendarColorId = CalendarColor;

/** One colour of the palette: id and value. */
export type { PaletteColor };

/**
 * The colours offered for calendars and task lists, in the order the core
 * hands out defaults to new and discovered calendars (R7, ADR 0246).
 */
export const CALENDAR_COLORS: readonly PaletteColor[] = CALENDAR_PALETTE;

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

/**
 * The OKLab distance (Euclidean, L 0–1) below which two calendar colours are
 * too close to tell apart on a card's bar or a swatch (R8, ADR 0251 item 5).
 * It stays under the closest pair of the palette (orange / red, ≈ 0.087), so
 * no two palette colours ever count as too close.
 */
export const MIN_COLOR_DISTANCE = 0.08;

/** A colour in OKLab: `[L, a, b]`. */
type Oklab = readonly [number, number, number];

/** `#rgb` / `#rrggbb` / `#rrggbbaa` (alpha ignored) → OKLab; keywords and anything else → `null`. */
export function oklabOf(value: string | null | undefined): Oklab | null {
  const v = (value ?? "").trim();
  const rgb = parseHex(v.length === 9 ? v.slice(0, 7) : v);
  if (!rgb) return null;
  const [r, g, b] = linearRgb(rgb);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

const labDistance = (x: Oklab, y: Oklab) => Math.hypot(x[0] - y[0], x[1] - y[1], x[2] - y[2]);

/** The perceptual (OKLab) distance of two hex colours; `null` when one is not hex. */
export function colorDistance(a: string, b: string): number | null {
  const x = oklabOf(a);
  const y = oklabOf(b);
  return x && y ? labDistance(x, y) : null;
}

/** A palette colour, also written `#rrggbbff` (what Nail itself stores and offers). */
function isPaletteValue(value: string): boolean {
  const v = value.trim().toLowerCase();
  return paletteColorOf(v.length === 9 && v.endsWith("ff") ? v.slice(0, 7) : v) !== null;
}

/**
 * The colour each calendar is shown in (R8, ADR 0251 item 5): its stored
 * colour, except that a visible calendar whose colour came from elsewhere (a
 * server's `calendar-color`, a subscription's setting: not a palette colour)
 * and is too close to the colour of another visible calendar shown before it
 * is shown in the first palette colour that is distinct from all of them.
 * Palette colours — what the user picks in Nail and what the core hands out
 * — are always shown as stored and count first, so a picked colour never
 * moves. Hidden calendars, colours without a value and keywords stay as
 * stored. Display only: nothing is written back.
 */
export function displayColors(
  calendars: readonly Calendar[],
): ReadonlyMap<CalendarId, string | null> {
  const out = new Map<CalendarId, string | null>();
  const placed: Oklab[] = [];
  const later: Calendar[] = [];
  for (const calendar of calendars) {
    const color = calendar.settings.color;
    out.set(calendar.id, color);
    if (!calendar.settings.visible || !isCssColor(color)) continue;
    const lab = oklabOf(color);
    if (lab === null) continue;
    if (isPaletteValue(color)) placed.push(lab);
    else later.push(calendar);
  }
  const distinct = (lab: Oklab) => placed.every((p) => labDistance(p, lab) >= MIN_COLOR_DISTANCE);
  for (const calendar of later) {
    const lab = oklabOf(calendar.settings.color);
    if (lab === null) continue;
    if (distinct(lab)) {
      placed.push(lab);
      continue;
    }
    const next = CALENDAR_COLORS.find((c) => {
      const p = oklabOf(c.value);
      return p !== null && distinct(p);
    });
    if (next === undefined) continue;
    out.set(calendar.id, next.value);
    const p = oklabOf(next.value);
    if (p) placed.push(p);
  }
  return out;
}

const DISPLAY_CACHE = new WeakMap<readonly Calendar[], ReadonlyMap<CalendarId, string | null>>();

/** {@link displayColors} of a calendar list, computed once per list (query data is stable). */
export function displayColorOf(
  calendars: readonly Calendar[] | undefined,
  calendarId: CalendarId,
): string | null {
  if (!calendars) return null;
  let colors = DISPLAY_CACHE.get(calendars);
  if (colors === undefined) {
    colors = displayColors(calendars);
    DISPLAY_CACHE.set(calendars, colors);
  }
  return colors.get(calendarId) ?? null;
}
