/**
 * Links in a mail frame (ADR 0206): no click ever navigates the frame. The
 * frame's script (`FRAME_SCRIPT`, ADR 0212; both frames run it) posts
 * `{type: FRAME_LINK_MESSAGE, open, href, index}` for the link under the
 * pointer or the keyboard focus and for a clicked one (a click, Enter on a
 * focused link, a middle click); the reader accepts it only from its own
 * frame's window ({@link frameLinkOf}) and follows the link itself
 * (`ReaderLinks.tsx`).
 *
 * A link's number is the core's (`nail_mime::links`, the `link` of a
 * `MessageWarning`): its position among the document's `a[href]` elements.
 */
import { FRAME_LINK_MESSAGE } from "../api/generated";
import { framePosted, type FrameMessageEvent } from "../theme/frameHeight";

/** A link of a mail document. */
export interface MailLink {
  /** Its `href` as written in the sanitized HTML. */
  href: string;
  /** Its number among the document's links (the core's numbering). */
  index: number;
}

/** What a frame reports: the link now hovered / focused (`null`: none), or a click on one. */
export interface MailLinkReport {
  link: MailLink | null;
  /** The link was clicked (or Enter / middle click): follow it. */
  open: boolean;
}

/** The middle mouse button (`MouseEvent.button`). */
export const MIDDLE_BUTTON = 1;

/**
 * The link report a mail frame posted, or `null` unless it comes
 * from `frame`'s own window and has the contract's shape.
 */
export function frameLinkOf(
  event: FrameMessageEvent,
  frame: Window | null,
  sandboxOrigin: string,
): MailLinkReport | null {
  const posted = framePosted(event, frame, sandboxOrigin, FRAME_LINK_MESSAGE);
  if (posted === null || typeof posted.open !== "boolean") return null;
  const { open, href, index } = posted;
  if (href === null && !open) return { link: null, open };
  if (typeof href !== "string" || typeof index !== "number" || !Number.isInteger(index)) {
    return null;
  }
  return { link: { href, index }, open };
}

/** Longest link target the status overlay shows in full. */
export const LINK_STATUS_CHARS = 80;

/** Shortens `text` in the middle to at most `max` characters (`…` in between). */
export function shortenMiddle(text: string, max = LINK_STATUS_CHARS): string {
  const chars = Array.from(text);
  if (chars.length <= max) return text;
  const keep = Math.max(0, max - 1);
  const head = Math.ceil(keep / 2);
  return `${chars.slice(0, head).join("")}…${chars.slice(chars.length - (keep - head)).join("")}`;
}
