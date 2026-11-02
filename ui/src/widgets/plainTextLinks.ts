/**
 * Links in plain-text mail (ADR 0206, 0211): a mail without an HTML part
 * shows its text in a `<pre>`, so the reader finds the links itself. Only the
 * schemes the core's `OpenLink` handles
 * (`EXTERNAL_LINK_SCHEMES` and `MAILTO_SCHEME`, generated from
 * `nail_api::links`) are links; a click goes the same way as in HTML mail.
 */
import { EXTERNAL_LINK_SCHEMES, MAILTO_SCHEME } from "../api/generated";

/** A stretch of a plain text: a link when `href` is set. */
export interface TextPart {
  text: string;
  href: string | null;
}

/** Schemes whose address follows `//` (the others, `mailto:` and `tel:`, do not). */
const AUTHORITY_SCHEMES: readonly string[] = ["http", "https"];

const SCHEMES: readonly string[] = [...EXTERNAL_LINK_SCHEMES, MAILTO_SCHEME];

/** A scheme at a word start, then everything up to white space, brackets and quotes. */
const LINK_PATTERN = new RegExp(`\\b(?:${SCHEMES.join("|")}):[^\\s<>"'\\u0000-\\u001f]+`, "giu");

/** Punctuation that ends a sentence rather than a link. */
const TRAILING = /[.,;:!?'"»”’]$/u;

const CLOSERS: Readonly<Record<string, string>> = { ")": "(", "]": "[", "}": "{" };

/** `candidate` without the sentence punctuation and unbalanced brackets at its end. */
function trimLink(candidate: string): string {
  let link = candidate;
  for (;;) {
    const last = link.slice(-1);
    const opener = CLOSERS[last];
    const unbalanced =
      opener !== undefined && link.split(last).length - 1 > link.split(opener).length - 1;
    if (TRAILING.test(link) || unbalanced) link = link.slice(0, -1);
    else return link;
  }
}

/** Whether `link` has a scheme and a body `OpenLink` can use. */
function isLink(link: string): boolean {
  const colon = link.indexOf(":");
  const scheme = link.slice(0, colon).toLowerCase();
  const rest = link.slice(colon + 1);
  if (AUTHORITY_SCHEMES.includes(scheme)) return rest.startsWith("//") && rest.length > 2;
  return rest.length > 0;
}

/** Splits `text` into plain stretches and links; the parts joined are `text`. */
export function linkify(text: string): TextPart[] {
  const parts: TextPart[] = [];
  let from = 0;
  for (const match of text.matchAll(LINK_PATTERN)) {
    const link = trimLink(match[0]);
    if (!isLink(link)) continue;
    if (match.index > from) parts.push({ text: text.slice(from, match.index), href: null });
    parts.push({ text: link, href: link });
    from = match.index + link.length;
  }
  if (from < text.length) parts.push({ text: text.slice(from), href: null });
  return parts;
}
