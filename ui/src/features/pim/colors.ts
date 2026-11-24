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
import type { CalendarColor, PaletteColor } from "../../api/generated";

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
