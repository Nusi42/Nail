/**
 * The text of a plain-text mail with its links clickable (ADR 0211): each
 * link is reported to the reader like a link of an HTML mail
 * (`MailLinkReport`, `ReaderLinks.tsx`), so a click asks the core
 * (`OpenLink`) and the hovered or focused link shows in the status overlay.
 * The core numbers only the links of an HTML document, so these have
 * {@link PLAIN_TEXT_LINK_INDEX} and are never flagged.
 */
import { Fragment, useMemo } from "react";
import { MIDDLE_BUTTON, type MailLinkReport } from "./mailLinks";
import { linkify } from "./plainTextLinks";

/** The number of a plain-text link (the core's numbers start at 0). */
export const PLAIN_TEXT_LINK_INDEX = -1;

export function LinkedText({
  text,
  onLink,
}: {
  text: string;
  onLink: (report: MailLinkReport) => void;
}) {
  const parts = useMemo(() => linkify(text), [text]);
  return (
    <>
      {parts.map((part, at) => {
        const { href } = part;
        if (href === null) return <Fragment key={at}>{part.text}</Fragment>;
        const link = { href, index: PLAIN_TEXT_LINK_INDEX };
        const show = () => {
          onLink({ link, open: false });
        };
        const hide = () => {
          onLink({ link: null, open: false });
        };
        return (
          <a
            key={at}
            data-part="message-view.text-link"
            href={href}
            rel="noreferrer noopener"
            onClick={(event) => {
              event.preventDefault();
              onLink({ link, open: true });
            }}
            onAuxClick={(event) => {
              event.preventDefault();
              if (event.button === MIDDLE_BUTTON) onLink({ link, open: true });
            }}
            onMouseEnter={show}
            onMouseLeave={hide}
            onFocus={show}
            onBlur={hide}
          >
            {part.text}
          </a>
        );
      })}
    </>
  );
}
