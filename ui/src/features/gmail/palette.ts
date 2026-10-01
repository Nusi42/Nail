/**
 * Gmail's label colours. The Gmail API accepts only colours of its own
 * palette (`users.labels`: `color.backgroundColor` / `color.textColor`);
 * anything else is refused by the server and comes back `rejected` (the
 * contract only checks `#rrggbb`, ADR 0065). The editor therefore offers
 * named background / text pairs taken from that palette only.
 */
import type { GmailLabelColor } from "../../api/generated";

/** Every colour the Gmail API accepts (background and text alike), lower case. */
export const GMAIL_PALETTE: ReadonlySet<string> = new Set([
  "#000000",
  "#434343",
  "#666666",
  "#999999",
  "#cccccc",
  "#efefef",
  "#f3f3f3",
  "#ffffff",
  "#fb4c2f",
  "#ffad47",
  "#fad165",
  "#16a766",
  "#43d692",
  "#4a86e8",
  "#a479e2",
  "#f691b3",
  "#f6c5be",
  "#ffe6c7",
  "#fef1d1",
  "#b9e4d0",
  "#c6f3de",
  "#c9daf8",
  "#e4d7f5",
  "#fcdee8",
  "#efa093",
  "#ffd6a2",
  "#fce8b3",
  "#89d3b2",
  "#a0eac9",
  "#a4c2f4",
  "#d0bcf1",
  "#fbc8d9",
  "#e66550",
  "#ffbc6b",
  "#fcda83",
  "#44b984",
  "#68dfa9",
  "#6d9eeb",
  "#b694e8",
  "#f7a7c0",
  "#cc3a21",
  "#eaa041",
  "#f2c960",
  "#149e60",
  "#3dc789",
  "#3c78d8",
  "#8e63ce",
  "#e07798",
  "#ac2b16",
  "#cf8933",
  "#d5ae49",
  "#0b804b",
  "#2a9c68",
  "#285bac",
  "#653e9b",
  "#b65775",
  "#822111",
  "#a46a21",
  "#aa8831",
  "#076239",
  "#1a764d",
  "#1c4587",
  "#41236d",
  "#83334c",
  "#464646",
  "#e7e7e7",
  "#0d3472",
  "#b6cff5",
  "#0d3b44",
  "#98d7e4",
  "#3d188e",
  "#e3d7ff",
  "#711a36",
  "#fbd3e0",
  "#8a1c0a",
  "#f2b2a8",
  "#7a2e0b",
  "#ffc8af",
  "#7a4706",
  "#ffdeb5",
  "#594c05",
  "#fbe983",
  "#684e07",
  "#fdedc1",
  "#0b4f30",
  "#b3efd3",
  "#04502e",
  "#a2dcc1",
  "#c2c2c2",
  "#4986e7",
  "#2da2bb",
  "#b99aff",
  "#994a64",
  "#f691b2",
  "#ff7537",
  "#ffad46",
  "#662e37",
  "#ebdbde",
  "#cca6ac",
  "#094228",
  "#42d692",
  "#16a765",
]);

/** A named colour pair the editor offers (`gmail-color-<name>`). */
export interface PaletteColor {
  name: string;
  color: GmailLabelColor;
}

const pair = (name: string, background: string, text: string): PaletteColor => ({
  name,
  color: { background, text },
});

/** The pairs the label editor offers, in display order (all from `GMAIL_PALETTE`). */
export const LABEL_COLORS: readonly PaletteColor[] = [
  pair("red", "#fb4c2f", "#ffffff"),
  pair("orange", "#ffad47", "#000000"),
  pair("yellow", "#fad165", "#000000"),
  pair("green", "#16a766", "#ffffff"),
  pair("teal", "#43d692", "#000000"),
  pair("blue", "#4a86e8", "#ffffff"),
  pair("purple", "#a479e2", "#ffffff"),
  pair("pink", "#f691b3", "#000000"),
  pair("light-red", "#f6c5be", "#ac2b16"),
  pair("light-orange", "#ffe6c7", "#a46a21"),
  pair("light-yellow", "#fef1d1", "#aa8831"),
  pair("light-green", "#b9e4d0", "#076239"),
  pair("light-teal", "#c6f3de", "#1a764d"),
  pair("light-blue", "#c9daf8", "#1c4587"),
  pair("light-purple", "#e4d7f5", "#41236d"),
  pair("light-pink", "#fcdee8", "#83334c"),
  pair("dark-red", "#822111", "#ffffff"),
  pair("dark-orange", "#a46a21", "#ffffff"),
  pair("dark-yellow", "#aa8831", "#ffffff"),
  pair("dark-green", "#076239", "#ffffff"),
  pair("dark-teal", "#1a764d", "#ffffff"),
  pair("dark-blue", "#1c4587", "#ffffff"),
  pair("dark-purple", "#41236d", "#ffffff"),
  pair("dark-pink", "#83334c", "#ffffff"),
  pair("gray", "#666666", "#ffffff"),
  pair("light-gray", "#efefef", "#000000"),
  pair("black", "#000000", "#ffffff"),
];

/** Whether Gmail accepts a colour pair. */
export function inPalette(color: GmailLabelColor): boolean {
  return (
    GMAIL_PALETTE.has(color.background.toLowerCase()) && GMAIL_PALETTE.has(color.text.toLowerCase())
  );
}

/** The named pair of a colour, if it is one of the offered ones. */
export function paletteName(color: GmailLabelColor | null): string | null {
  if (!color) return null;
  return (
    LABEL_COLORS.find(
      (p) =>
        p.color.background === color.background.toLowerCase() &&
        p.color.text === color.text.toLowerCase(),
    )?.name ?? null
  );
}
