/**
 * The composer ↔ `Draft` mapping for all three modes.
 */
import type { Draft } from "../../api/generated";
import { formatAddressList, parseAddressList } from "../../state/addresses";
import { emptyComposer } from "../../state/compose";
import type { ComposerState } from "../../state/uiStore";
import { composerCrypto } from "../security";
import type { RichState } from "./store";

/** Content ids in `cid:` references of an HTML body. */
export function referencedCids(html: string): Set<string> {
  const out = new Set<string>();
  for (const match of html.matchAll(/cid:([^"'\s)>]+)/gi)) out.add((match[1] ?? "").toLowerCase());
  return out;
}

/** HTML as stored (`cid:` references) → HTML for the editor (local previews). */
export function toDisplayHtml(html: string, previews: Readonly<Record<string, string>>): string {
  let out = html;
  for (const [cid, url] of Object.entries(previews)) out = out.replaceAll(`cid:${cid}`, url);
  return out;
}

/** HTML from the editor → HTML as stored (previews back to `cid:`). */
export function toStoredHtml(html: string, previews: Readonly<Record<string, string>>): string {
  let out = html;
  for (const [cid, url] of Object.entries(previews)) out = out.replaceAll(url, `cid:${cid}`);
  return out;
}

/** Builds the draft of the composer; `invalid` lists unparsable recipients. */
export function richDraftOf(c: ComposerState, r: RichState): { draft: Draft; invalid: string[] } {
  const to = parseAddressList(c.to);
  const cc = parseAddressList(c.cc);
  const bcc = parseAddressList(c.bcc);
  // The sign / encrypt toggles (M2a): only an explicit choice is saved; an
  // untouched composer follows the identity defaults and Autocrypt.
  const crypto = composerCrypto();
  return {
    draft: {
      id: c.draftId,
      identity_id: c.identityId ?? "",
      to: to.addresses,
      cc: cc.addresses,
      bcc: bcc.addresses,
      subject: c.subject,
      mode: r.mode,
      text: r.mode === "plain" ? c.text : "",
      html: r.mode === "html" ? r.html : null,
      markdown: r.mode === "markdown" ? r.markdown : null,
      attachments: r.attachments,
      context: c.context,
      signature: r.signature,
      markdown_part: r.mode === "markdown" && r.markdownPart,
      ...(crypto ? { crypto } : {}),
    },
    invalid: [...to.invalid, ...cc.invalid, ...bcc.invalid],
  };
}

/** A stable key of what would be saved (autosave compares keys). */
export function draftKey(c: ComposerState, r: RichState): string {
  const { draft } = richDraftOf(c, r);
  return JSON.stringify({ ...draft, id: null, to: c.to, cc: c.cc, bcc: c.bcc });
}

/** The body source of the current mode. */
export function bodyOf(c: ComposerState, r: RichState): string {
  switch (r.mode) {
    case "html":
      return r.html;
    case "markdown":
      return r.markdown;
    case "plain":
      return c.text;
  }
}

/** Composer + rich state from a draft (NewDraft, GetDraft). */
export function fromDraft(
  draft: Draft,
  base: Partial<ComposerState> = {},
): { composer: ComposerState; rich: Partial<RichState> } {
  const mode = draft.mode;
  return {
    composer: {
      ...emptyComposer(),
      draftId: draft.id,
      identityId: draft.identity_id === "" ? null : draft.identity_id,
      to: formatAddressList(draft.to),
      cc: formatAddressList(draft.cc),
      bcc: formatAddressList(draft.bcc),
      showCc: draft.cc.length > 0,
      showBcc: draft.bcc.length > 0,
      subject: draft.subject,
      text: mode === "plain" ? draft.text : "",
      context: draft.context,
      ...base,
    },
    rich: {
      mode,
      html: draft.html ?? "",
      markdown: draft.markdown ?? "",
      attachments: draft.attachments,
      markdownPart: draft.markdown_part,
      signature: draft.signature,
    },
  };
}
