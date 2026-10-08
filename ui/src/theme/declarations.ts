/**
 * CSS declarations and lengths several skins share (Q2, SSOT): one
 * definition per look, so the skins cannot drift apart. Every value is a
 * `var(--nail-…)` token where the token schema has one; the lengths here
 * have no token (yet) and are named once instead of repeated.
 *
 * The skins interpolate these into their rules; `declarations.test.ts`
 * checks that each is a well-formed declaration list.
 */

const LABEL = "var(--nail-font-size-label)";
const SEMIBOLD = "var(--nail-font-weight-semibold)";
const MUTED = "var(--nail-color-fg-muted)";

/** Small caps for section labels (menu headers, settings groups, calendar and contact sections). */
export const CAPS = "letter-spacing: 0.05em; text-transform: uppercase;";

/** Small caps, a little tighter: folder-pane sections, toolbar groups, the undo button. */
export const CAPS_TIGHT = "letter-spacing: 0.04em; text-transform: uppercase;";

/** A section label: muted small caps in the label size. */
export const SECTION_LABEL = `font-size: ${LABEL}; font-weight: ${SEMIBOLD}; ${CAPS} color: ${MUTED};`;

/** The smallest text: a step below the label size (badge counts, file-type badges). */
export const TINY_TEXT = "calc(var(--nail-font-size-label) - 1.5px)";

/**
 * The text on palette colours (avatar initials, file-type labels): white,
 * unless a theme sets `color "avatar-fg"` (High contrast: black).
 */
export const AVATAR_FG = "var(--nail-color-avatar-fg, #ffffff)";

/** Initials on avatars and file-type badges, spaced a little apart. */
export const INITIALS_TRACKING = "letter-spacing: 0.02em;";

/** A round avatar `size` wide and high, its initials centred. */
export function avatarDisk(size: string): string {
  return `display: grid; place-items: center; width: ${size}; height: ${size}; border-radius: 50%;`;
}

/** A round avatar with initials in `fontSize`, in the avatar text colour. */
export function initialsAvatar(size: string, fontSize: string): string {
  return `${avatarDisk(size)}
  font-size: ${fontSize}; font-weight: ${SEMIBOLD}; line-height: 1; ${INITIALS_TRACKING}
  color: ${AVATAR_FG}; user-select: none;`;
}

/**
 * A small tag beside a title (a pane's kind, a calendar's kind): label-size
 * text on its own background, never shrinking.
 */
export function smallTag(radius: string, color: string, background: string): string {
  return `flex: 0 0 auto; padding: 0 var(--nail-space-xs); border-radius: ${radius};
  font-size: ${LABEL}; color: ${color}; background-color: ${background};`;
}

/**
 * Visually hidden, still read by screen readers (the accessible name stays):
 * a clipped 1px box out of the flow.
 */
export const VISUALLY_HIDDEN =
  "position: absolute; width: 1px; height: 1px; margin: -1px; padding: 0; overflow: hidden; clip-path: inset(50%); white-space: nowrap; border: 0;";

/** A row of inline items centred on one line. */
export const INLINE_ROW = "display: inline-flex; align-items: center;";

/** A quiet (borderless, muted) small button. */
export const QUIET_BUTTON =
  "border: none; border-radius: var(--nail-radius-sm); color: var(--nail-color-fg-muted); background-color: transparent;";

/** A quiet (borderless) button under the pointer. */
export const QUIET_HOVER =
  "color: var(--nail-color-fg); background-color: var(--nail-color-bg-raised);";

/** The height of one line of text: rows that must not collapse while empty. */
export const LINE_BOX = "1.5em";

/** The gap between inline items that scale with the text (shortcut keys, print header items). */
export const INLINE_GAP = "0.5em";

/** The bar beside quoted text and the current item (quotes, event colours). */
export const ACCENT_BAR = "3px";

/** High contrast: the focus outline of the selected row in a focused list. */
export const HC_RING = "3px";

/** Floating notices keep this far from the window's edges. */
export const FLOAT_INSET = "16px";

/** The width of a floating notice: at most `max`, never closer to the edges than {@link FLOAT_INSET}. */
export function floatingWidth(max: string): string {
  return `min(${max}, calc(100% - 2 * ${FLOAT_INSET}))`;
}

/** A stack of floating notices in the bottom `side` corner (plug-in, PIM and agent notices). */
export function noticeStack(side: "left" | "right"): string {
  return `position: fixed; ${side}: ${FLOAT_INSET}; bottom: ${FLOAT_INSET}; max-width: ${floatingWidth("420px")};`;
}

/** The reader's measure: plain-text mail and the conversation strip never get wider. */
export const READER_MEASURE = "68ch";

/** Below this width the reader (`nail-reader` container) uses the pane's padding instead of wide margins. */
export const READER_NARROW = "640px";
