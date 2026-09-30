/**
 * Composer flows: new message, reply / reply all / forward, save, send with
 * undo-send or at a later time, undo, discard. The composer's fields live in
 * the UI store (`composer`); drafts and the outbox are the core's.
 */
import { NailError, errorMessageIdOf } from "../api/client";
import type { AccountId, Draft, DraftId, Identity, MessageId } from "../api/generated";
import type { MessageArgs } from "../i18n/localizer";
import { displayName, formatAddressList, parseAddressList } from "./addresses";
import type { WriteContext } from "./mailOps";
import { queryKeys } from "./queries";
import type { ComposerState } from "./uiStore";
import { DATE_TIME, formatDateTime } from "../i18n/format";

/** Name of the layout window that holds the composer. */
export const COMPOSER_WINDOW = "composer";

export interface ComposeContext extends WriteContext {
  t: (id: string, args?: MessageArgs) => string;
  locale: string;
}

/** Subject prefixes (protocol conventions, not UI text). */
const REPLY_PREFIX = "Re: ";
const FORWARD_PREFIX = "Fwd: ";
const SIGNATURE_SEPARATOR = "-- ";

export function emptyComposer(): ComposerState {
  return {
    draftId: null,
    identityId: null,
    to: "",
    cc: "",
    bcc: "",
    subject: "",
    text: "",
    context: null,
    showCc: false,
    showBcc: false,
    identityPicked: false,
    busy: null,
    error: null,
  };
}

async function identitiesOf(ctx: ComposeContext, accountId: AccountId | null): Promise<Identity[]> {
  return ctx.queryClient.fetchQuery({
    queryKey: queryKeys.identities(accountId),
    queryFn: async () =>
      (await ctx.client.request({ type: "ListIdentities", account_id: accountId }, "Identities"))
        .identities,
  });
}

/**
 * The identity to send from, as the core chooses it: the virtual account's
 * identity rules, else the identity a replied-to message was sent to, else
 * the account's default.
 */
export async function chooseIdentity(
  ctx: ComposeContext,
  accountId: AccountId | null,
  recipients: readonly string[],
  replyTo: MessageId | null,
): Promise<Identity | null> {
  const { identity_id } = await ctx.client.request(
    {
      type: "ChooseIdentity",
      account_id: accountId,
      recipients: [...recipients],
      reply_to: replyTo,
    },
    "IdentityChosen",
  );
  if (identity_id === null) return null;
  return (await identitiesOf(ctx, null)).find((i) => i.id === identity_id) ?? null;
}

function signatureBlock(identity: Identity | null | undefined): string {
  const text = identity?.signature?.text;
  return text ? `\n\n${SIGNATURE_SEPARATOR}\n${text}` : "";
}

function open(ctx: ComposeContext, composer: ComposerState): void {
  const ui = ctx.store.getState();
  ui.setComposer(composer);
  ui.openWindow(COMPOSER_WINDOW);
}

/** Opens the composer; an unfinished message is resumed instead of replaced. */
export async function startCompose(ctx: ComposeContext): Promise<void> {
  const current = ctx.store.getState().composer;
  if (current) {
    ctx.store.getState().openWindow(COMPOSER_WINDOW);
    return;
  }
  const identity = await chooseIdentity(ctx, ctx.store.getState().activeAccountId, [], null);
  open(ctx, {
    ...emptyComposer(),
    identityId: identity?.id ?? null,
    text: signatureBlock(identity),
  });
}

export type ReplyMode = "reply" | "reply-all" | "forward";

function withPrefix(subject: string, prefix: string): string {
  return subject.toLowerCase().startsWith(prefix.trim().toLowerCase())
    ? subject
    : `${prefix}${subject}`;
}

function formatDate(ctx: ComposeContext, date: string | null): string {
  if (!date) return "";
  return formatDateTime(date, ctx.locale, DATE_TIME);
}

function quote(text: string): string {
  return text
    .split("\n")
    .map((line) => (line.startsWith(">") ? `>${line}` : `> ${line}`))
    .join("\n");
}

/** Opens the composer to reply to or forward a message. */
export async function startReply(
  ctx: ComposeContext,
  messageId: MessageId,
  mode: ReplyMode,
): Promise<void> {
  const message = await ctx.queryClient.fetchQuery({
    queryKey: queryKeys.message(messageId),
    queryFn: async () =>
      (await ctx.client.request({ type: "GetMessage", id: messageId }, "Message")).message,
  });
  const identities = await identitiesOf(ctx, null);
  const accountId = ctx.store.getState().activeAccountId;
  const own = new Set(identities.map((i) => i.email.toLowerCase()));
  const h = message.headers;
  const answered = h.reply_to.length > 0 ? h.reply_to : h.from;
  const recipients =
    mode === "forward" ? [] : [...answered, ...(mode === "reply-all" ? [...h.to, ...h.cc] : [])];
  const identity = await chooseIdentity(
    ctx,
    accountId,
    recipients.map((a) => a.email),
    message.id,
  );
  const composer = emptyComposer();
  composer.identityId = identity?.id ?? null;
  const sender = h.from[0];

  if (mode === "forward") {
    composer.subject = withPrefix(h.subject, FORWARD_PREFIX);
    const lines = [
      ctx.t("composer-forward-header"),
      `${ctx.t("message-header-from")}: ${formatAddressList(h.from)}`,
      `${ctx.t("message-header-date")}: ${formatDate(ctx, h.date)}`,
      `${ctx.t("composer-field-subject")}: ${h.subject}`,
      `${ctx.t("message-header-to")}: ${formatAddressList(h.to)}`,
    ];
    composer.text = `${signatureBlock(identity)}\n\n${lines.join("\n")}\n\n${message.text}`;
    composer.context = { mode: "forward", message_id: message.id };
  } else {
    composer.context = { mode, message_id: message.id };
    composer.subject = withPrefix(h.subject, REPLY_PREFIX);
    const replyTo = h.reply_to.length > 0 ? h.reply_to : h.from;
    composer.to = formatAddressList(replyTo);
    if (mode === "reply-all") {
      const seen = new Set(replyTo.map((a) => a.email.toLowerCase()));
      const cc = [...h.to, ...h.cc].filter((a) => {
        const key = a.email.toLowerCase();
        if (seen.has(key) || own.has(key)) return false;
        seen.add(key);
        return true;
      });
      composer.cc = formatAddressList(cc);
      composer.showCc = cc.length > 0;
    }
    const header = ctx.t("composer-quote-header", {
      date: formatDate(ctx, h.date),
      name: displayName(sender) || ctx.t("composer-unknown-sender"),
    });
    composer.text = `\n\n${header}\n${quote(message.text)}${signatureBlock(identity)}`;
  }
  open(ctx, composer);
}

/** Builds the draft of the composer; `invalid` lists unparsable recipients. */
export function draftOf(c: ComposerState): { draft: Draft; invalid: string[] } {
  const to = parseAddressList(c.to);
  const cc = parseAddressList(c.cc);
  const bcc = parseAddressList(c.bcc);
  return {
    draft: {
      id: c.draftId,
      identity_id: c.identityId ?? "",
      to: to.addresses,
      cc: cc.addresses,
      bcc: bcc.addresses,
      subject: c.subject,
      mode: "plain",
      text: c.text,
      html: null,
      markdown: null,
      attachments: [],
      context: c.context,
      signature: "identity",
      markdown_part: false,
    },
    invalid: [...to.invalid, ...cc.invalid, ...bcc.invalid],
  };
}

function fail(ctx: ComposeContext, id: string, args?: Record<string, string | number>): void {
  ctx.store.getState().updateComposer({ busy: null, error: { id, ...(args ? { args } : {}) } });
}

/** Saves the composer as a draft; returns its id, or `null` on failure. */
export async function saveComposer(ctx: ComposeContext): Promise<DraftId | null> {
  const composer = ctx.store.getState().composer;
  if (!composer) return null;
  if (composer.identityId === null) {
    fail(ctx, "composer-error-no-identity");
    return null;
  }
  ctx.store.getState().updateComposer({ busy: "saving", error: null });
  try {
    const { draft_id } = await ctx.client.request(
      { type: "SaveDraft", draft: draftOf(composer).draft },
      "DraftSaved",
    );
    ctx.store.getState().updateComposer({ draftId: draft_id, busy: null });
    return draft_id;
  } catch (error) {
    fail(ctx, errorMessageIdOf(error));
    return null;
  }
}

/**
 * Sends the composer's message: now plus the undo-send delay, or at
 * `sendAt` (RFC 3339). On success the composer closes and the status bar
 * shows the countdown.
 */
export async function sendComposer(ctx: ComposeContext, sendAt: string | null): Promise<boolean> {
  const composer = ctx.store.getState().composer;
  if (!composer || composer.busy !== null) return false;
  const { draft, invalid } = draftOf(composer);
  if (invalid.length > 0) {
    fail(ctx, "composer-error-invalid-address", { addresses: invalid.join(", ") });
    return false;
  }
  if (draft.to.length + draft.cc.length + draft.bcc.length === 0) {
    fail(ctx, "composer-error-no-recipients");
    return false;
  }
  if (sendAt !== null && Number.isNaN(Date.parse(sendAt))) {
    fail(ctx, "composer-error-send-at");
    return false;
  }
  const draftId = await saveComposer(ctx);
  if (draftId === null) return false;
  ctx.store.getState().updateComposer({ busy: "sending" });
  try {
    const { entry } = await ctx.client.request(
      { type: "SendDraft", draft_id: draftId, delay_secs: null, send_at: sendAt },
      "SendQueued",
    );
    const ui = ctx.store.getState();
    ui.setLastSend(entry);
    ui.setComposer(null);
    ui.closeWindow(COMPOSER_WINDOW);
    void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.outbox });
    return true;
  } catch (error) {
    fail(ctx, errorMessageIdOf(error));
    return false;
  }
}

/** Throws the message away (and its saved draft). */
export async function discardComposer(ctx: ComposeContext): Promise<void> {
  const composer = ctx.store.getState().composer;
  const ui = ctx.store.getState();
  ui.setComposer(null);
  ui.closeWindow(COMPOSER_WINDOW);
  if (composer?.draftId) {
    try {
      await ctx.client.request({ type: "DeleteDraft", draft_id: composer.draftId }, "Done");
    } catch (error) {
      if (!(error instanceof NailError && error.code === "not_found")) throw error;
    }
  }
}

/** Opens a saved draft in the composer. */
export async function openDraft(ctx: ComposeContext, draftId: DraftId): Promise<void> {
  const { draft } = await ctx.client.request({ type: "GetDraft", draft_id: draftId }, "Draft");
  open(ctx, {
    ...emptyComposer(),
    draftId: draft.id ?? draftId,
    identityId: draft.identity_id,
    identityPicked: true,
    to: formatAddressList(draft.to),
    cc: formatAddressList(draft.cc),
    bcc: formatAddressList(draft.bcc),
    showCc: draft.cc.length > 0,
    showBcc: draft.bcc.length > 0,
    subject: draft.subject,
    text: draft.text,
    context: draft.context,
  });
}

/**
 * Takes a message back out of the outbox (undo send, or cancelling a
 * scheduled message) and reopens it in the composer.
 */
export async function cancelSend(
  ctx: ComposeContext,
  outboxId: string,
  reopen: (ctx: ComposeContext, draftId: DraftId) => Promise<void> = openDraft,
): Promise<void> {
  const { draft_id } = await ctx.client.request(
    { type: "CancelSend", outbox_id: outboxId },
    "SendCancelled",
  );
  const ui = ctx.store.getState();
  if (ui.lastSend?.id === outboxId) ui.setLastSend(null);
  void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.outbox });
  void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.drafts });
  await reopen(ctx, draft_id);
}
