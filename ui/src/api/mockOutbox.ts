/**
 * Drafts and the outbox of the mock core. `SendDraft` schedules the entry
 * (undo-send delay or send-later time) with real timers, so tests drive it
 * with fake timers; sending emits `SendStatus` events and files a copy in
 * the account's Sent folder.
 */
import { htmlToText, saveRichDraft } from "../features/composer/mock";
import { checkSendCrypto } from "../features/security/mock";
import type { Draft, OutboxEntry, Response, SendCrypto } from "./generated";
import { mockBody } from "./mockData";
import { errorResponse, type MockCore } from "./mockMail";

/** Default undo-send delay (`AccountsConfig::DEFAULT_UNDO_SEND_SECS`). */
export const DEFAULT_UNDO_SEND_SECS = 10;

const timers = new WeakMap<OutboxEntry, ReturnType<typeof setTimeout>>();

/** Longest delay `setTimeout` supports (larger ones fire at once). */
const MAX_TIMEOUT_MS = 2 ** 31 - 1;

/** Runs `fn` at `due` (ms since the epoch), even months ahead. */
function at(entry: OutboxEntry, due: number, fn: () => void): void {
  const wait = due - Date.now();
  timers.set(
    entry,
    wait > MAX_TIMEOUT_MS
      ? setTimeout(() => at(entry, due, fn), MAX_TIMEOUT_MS)
      : setTimeout(fn, Math.max(0, wait)),
  );
}

function identityOf(core: MockCore, id: string) {
  for (const account of core.data.accountsConfig.accounts) {
    const identity = account.identities.find((i) => i.id === id);
    if (identity) return identity;
  }
  return undefined;
}

export function saveDraft(core: MockCore, draft: Draft): Response {
  if (draft.html !== null || draft.markdown !== null || draft.attachments.length > 0) {
    return saveRichDraft(core, draft);
  }
  if (!identityOf(core, draft.identity_id)) {
    return errorResponse("invalid_request", `unknown identity ${draft.identity_id}`);
  }
  if (draft.id !== null && !core.data.drafts.has(draft.id)) {
    return errorResponse("not_found", `draft ${draft.id} not found`);
  }
  const id = draft.id ?? core.nextId("draft-");
  core.data.drafts.set(id, { ...draft, id });
  return { type: "DraftSaved", draft_id: id };
}

export function getDraft(core: MockCore, id: string): Response {
  const draft = core.data.drafts.get(id);
  return draft ? { type: "Draft", draft } : errorResponse("not_found", `draft ${id} not found`);
}

export function deleteDraft(core: MockCore, id: string): Response {
  if (!core.data.drafts.delete(id)) return errorResponse("not_found", `draft ${id} not found`);
  return { type: "Done" };
}

function setState(core: MockCore, entry: OutboxEntry, state: OutboxEntry["state"]): void {
  entry.state = state;
  core.emit({
    type: "SendStatus",
    outbox_id: entry.id,
    state,
    error: entry.error,
    message: entry.message,
  });
}

function deliver(core: MockCore, entry: OutboxEntry, draft: Draft): void {
  const d = core.data;
  setState(core, entry, "sending");
  core.later(() => {
    if (entry.state !== "sending") return;
    entry.attempts++;
    const error = d.behaviour.sendError;
    if (error) {
      entry.error = error;
      entry.message = "mock submission failed";
      setState(core, entry, "failed");
      return;
    }
    const sent = d.folders.find((f) => f.account_id === entry.account_id && f.role === "sent");
    const identity = identityOf(core, draft.identity_id);
    if (sent && identity) {
      const body = draft.markdown ?? (draft.html !== null ? htmlToText(draft.html) : draft.text);
      const summary = {
        id: `${sent.id}#${entry.id}`,
        folder_id: sent.id,
        account_id: entry.account_id,
        subject: draft.subject,
        from: { name: identity.name, email: identity.email },
        to: draft.to,
        date: new Date().toISOString(),
        flags: { seen: true, flagged: false, answered: false, draft: false },
        has_attachments: draft.attachments.length > 0,
        preview: body.replace(/\s+/g, " ").slice(0, 120),
        thread_id: null,
        tags: [],
        duplicates: [],
      };
      d.messages.unshift(summary);
      d.bodies.set(summary.id, mockBody(summary, { text: body }));
      core.emit({ type: "MailChanged", folder_ids: [sent.id] });
    }
    const replied =
      draft.context && draft.context.mode !== "forward" ? draft.context.message_id : null;
    if (replied) {
      const original = d.messages.find((m) => m.id === replied);
      if (original) original.flags = { ...original.flags, answered: true };
    }
    d.drafts.delete(entry.draft_id);
    setState(core, entry, "sent");
    d.outbox = d.outbox.filter((e) => e !== entry);
  });
}

export function sendDraft(
  core: MockCore,
  draftId: string,
  delaySecs: number | null,
  sendAt: string | null,
  crypto?: SendCrypto,
): Response {
  const d = core.data;
  const draft = d.drafts.get(draftId);
  if (!draft) return errorResponse("not_found", `draft ${draftId} not found`);
  if (draft.to.length + draft.cc.length + draft.bcc.length === 0) {
    return errorResponse("invalid_request", "the draft has no recipients");
  }
  const identity = identityOf(core, draft.identity_id);
  if (!identity) return errorResponse("invalid_request", "unknown identity");
  // Like the core (ADRs 0029, 0034): sign / encrypt from the request, the
  // draft or the identity; refused with `no_key` / `locked` when it cannot
  // work, never sent plain instead.
  const wanted: SendCrypto = crypto ??
    draft.crypto ?? {
      sign: identity.crypto?.sign ?? false,
      encrypt: identity.crypto?.encrypt ?? false,
      technology: null,
      protect_headers: null,
    };
  if (wanted.sign || wanted.encrypt) {
    const recipients = [...draft.to, ...draft.cc, ...draft.bcc].map((a) => a.email);
    const refused = checkSendCrypto(core, identity, recipients, wanted);
    if (refused) return refused;
  }
  const now = Date.now();
  let due: number;
  if (sendAt !== null) {
    due = Date.parse(sendAt);
    if (Number.isNaN(due)) return errorResponse("invalid_request", "invalid send_at");
  } else {
    const delay = delaySecs ?? d.accountsConfig.undo_send_secs ?? DEFAULT_UNDO_SEND_SECS;
    due = now + Math.max(0, delay) * 1000;
  }
  const entry: OutboxEntry = {
    id: core.nextId("out-"),
    draft_id: draftId,
    account_id: identity.account_id,
    identity_id: identity.id,
    subject: draft.subject,
    recipients: [...draft.to, ...draft.cc, ...draft.bcc],
    send_at: new Date(due).toISOString(),
    state: "scheduled",
    attempts: 0,
    error: null,
    message: null,
  };
  d.outbox.push(entry);
  d.outbox.sort((a, b) => Date.parse(a.send_at) - Date.parse(b.send_at));
  at(entry, due, () => deliver(core, entry, draft));
  return { type: "SendQueued", entry: { ...entry } };
}

export function cancelSend(core: MockCore, outboxId: string): Response {
  const d = core.data;
  const entry = d.outbox.find((e) => e.id === outboxId);
  if (!entry) return errorResponse("not_found", `outbox entry ${outboxId} not found`);
  if (entry.state !== "scheduled" && entry.state !== "failed") {
    return errorResponse("conflict", "sending already started");
  }
  clearTimeout(timers.get(entry));
  d.outbox = d.outbox.filter((e) => e !== entry);
  setState(core, entry, "cancelled");
  return { type: "SendCancelled", draft_id: entry.draft_id };
}

/** `ListDrafts`: saved drafts not waiting in the outbox, newest first. */
export function listDrafts(core: MockCore): Response {
  const queued = new Set(core.data.outbox.map((e) => e.draft_id));
  const drafts = [...core.data.drafts.values()]
    .filter((d) => d.id === null || !queued.has(d.id))
    .reverse()
    .map((d) => structuredClone(d));
  return { type: "Drafts", drafts };
}

export function listOutbox(core: MockCore): Response {
  return { type: "Outbox", entries: core.data.outbox.map((e) => ({ ...e })) };
}
