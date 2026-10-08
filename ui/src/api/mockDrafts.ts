/**
 * The mock core's checks every `SaveDraft` makes, plain and rich drafts
 * alike (`api/mockOutbox.ts`, `features/composer/mock.ts`).
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
