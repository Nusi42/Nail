/**
 * The mock core's checks every `SaveDraft` makes, plain and rich drafts
 * alike (`api/mockOutbox.ts`, `features/composer/mock.ts`), and
 * `EditDraftMessage`.
 */
import type { Draft, Identity, Response } from "./generated";
import { errorResponse, type MockCore } from "./mockMail";

/** The identity `id` of any account. */
export function identityOf(core: MockCore, id: string): Identity | undefined {
  for (const account of core.data.accountsConfig.accounts) {
    const identity = account.identities.find((i) => i.id === id);
    if (identity) return identity;
  }
  return undefined;
}

/** Why `SaveDraft` refuses `draft` (an unknown identity or draft), `null` when it does not. */
export function draftSaveRefusal(core: MockCore, draft: Draft): Response | null {
  if (!identityOf(core, draft.identity_id)) {
    return errorResponse("invalid_request", `unknown identity ${draft.identity_id}`);
  }
  if (draft.id !== null && !core.data.drafts.has(draft.id)) {
    return errorResponse("not_found", `draft ${draft.id} not found`);
  }
  return null;
}

/**
 * `EditDraftMessage`: the draft a message of a Drafts folder continues. The
 * mock keeps no server copies, so a message becomes a new draft (sender
 * identity, recipients, subject, body) the first time and names it after.
 * Like the core, only a `\Draft` message is linked to its draft; any other
 * message becomes a new draft every time (ADR 0232).
 */
export function editDraftMessage(core: MockCore, messageId: string): Response {
  const d = core.data;
  const known = d.draftMessages.get(messageId);
  if (known !== undefined && d.drafts.has(known)) return { type: "DraftSaved", draft_id: known };
  const summary = d.messages.find((m) => m.id === messageId);
  const body = d.bodies.get(messageId);
  if (!summary || !body) return errorResponse("not_found", `message ${messageId} not found`);
  const identities =
    d.accountsConfig.accounts.find((a) => a.id === summary.account_id)?.identities ?? [];
  const sender = body.headers.from[0]?.email.toLowerCase();
  const identity = identities.find((i) => i.email.toLowerCase() === sender) ?? identities[0];
  if (!identity) return errorResponse("invalid_request", `no identity for ${summary.account_id}`);
  const id = core.nextId("draft-");
  d.drafts.set(id, {
    id,
    identity_id: identity.id,
    to: body.headers.to,
    cc: body.headers.cc,
    bcc: [],
    subject: body.headers.subject,
    mode: body.sanitized_html === null ? "plain" : "html",
    text: body.text,
    html: body.sanitized_html,
    markdown: null,
    attachments: [],
    context: null,
    signature: "none",
    markdown_part: false,
  });
  if (summary.flags.draft) d.draftMessages.set(messageId, id);
  return { type: "DraftSaved", draft_id: id };
}
