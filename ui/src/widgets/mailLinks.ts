/**
 * Links in a mail frame (ADR 0206): no click ever navigates the frame. The
 * reader learns which link is under the pointer or the keyboard focus and
 * which one was clicked (a click, Enter on a focused link, a middle click),
 * and follows it itself (`ReaderLinks.tsx`).
 *
 * - Local frames (`srcdoc`, `sandbox="allow-same-origin"`, no scripts): the
 *   reader listens on the document ({@link watchLinks}).
 * - Remote frames (the remote UI's sandbox origin): the route's hash-pinned
 *   script does the same and posts `{type: FRAME_LINK_MESSAGE, open, href,
 *   index}`; the reader accepts it only from its own frame's window
 *   ({@link frameLinkOf}). The script's logic is {@link watchLinks}'
 *   (`crates/nail-server/src/sandbox.rs`, `FRAME_SCRIPT`).
 *
 * A link's number is the core's (`nail_mime::links`, the `link` of a
 * `MessageWarning`): its position among the document's `a[href]` elements.
 */
import { FRAME_LINK_MESSAGE } from "../api/generated";
import { framePosted, type FrameMessageEvent } from "../theme/frameHeight";

/** The selector of a link, as the core counts links. */
export const LINK_SELECTOR = "a[href]";

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

/** The link an event happened in (`closest("a[href]")`); works across frame realms. */
export function anchorOf(target: EventTarget | null): Element | null {
  const element = target as Partial<Element> | null;
  if (element === null || typeof element.closest !== "function") return null;
  return element.closest(LINK_SELECTOR);
}

/** A link element as a {@link MailLink}. */
export function mailLinkOf(doc: Document, anchor: Element): MailLink {
  return {
    href: anchor.getAttribute("href") ?? "",
    index: Array.prototype.indexOf.call(doc.querySelectorAll(LINK_SELECTOR), anchor),
  };
}

/** The middle mouse button (`MouseEvent.button`). */
const MIDDLE_BUTTON = 1;

/**
 * Watches the links of a local mail document: clicks on a link (also
 * middle clicks; Enter on a focused link is a click) never navigate and are
 * reported with `open`; the link under the pointer or the focus is reported
 * whenever it changes. Returns the function that stops watching.
 */
export function watchLinks(doc: Document, report: (r: MailLinkReport) => void): () => void {
  let current: Element | null = null;
  const show = (anchor: Element | null) => {
    if (anchor === current) return;
    current = anchor;
    report({ link: anchor === null ? null : mailLinkOf(doc, anchor), open: false });
  };
  const click = (event: Event) => {
    const anchor = anchorOf(event.target);
    if (anchor === null) return;
    event.preventDefault();
    if (event.type === "click" || (event as MouseEvent).button === MIDDLE_BUTTON) {
      report({ link: mailLinkOf(doc, anchor), open: true });
    }
  };
  const enter = (event: Event) => {
    show(anchorOf(event.target));
  };
  // Leaving the document (to the reader or elsewhere): nothing is hovered.
  const leave = (event: Event) => {
    if ((event as MouseEvent | FocusEvent).relatedTarget === null) show(null);
  };
  const listeners: [string, (event: Event) => void][] = [
    ["click", click],
    ["auxclick", click],
    ["mouseover", enter],
    ["focusin", enter],
    ["mouseout", leave],
    ["focusout", leave],
  ];
  for (const [type, listener] of listeners) doc.addEventListener(type, listener, true);
  return () => {
    for (const [type, listener] of listeners) doc.removeEventListener(type, listener, true);
  };
}

/**
 * The link report a sandbox-origin frame posted, or `null` unless it comes
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
