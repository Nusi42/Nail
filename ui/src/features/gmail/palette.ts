/**
 * Gmail's label colours. The Gmail API accepts only colours of its own
 * palette (`users.labels`: `color.backgroundColor` / `color.textColor`);
 * anything else is refused by the server and comes back `rejected` (the
 * contract only checks `#rrggbb`, ADR 0065). The editor therefore offers
 * named background / text pairs taken from that palette only.
 */
import { GMAIL_LABEL_PALETTE, type GmailLabelColor } from "../../api/generated";

/** Every colour the Gmail API accepts (background and text alike), lower case (`nail_api::gmail::LABEL_PALETTE`). */
export const GMAIL_PALETTE: ReadonlySet<string> = new Set(GMAIL_LABEL_PALETTE);

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
