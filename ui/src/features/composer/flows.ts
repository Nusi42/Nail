/**
 * Rich composer flows: new message / reply / forward through the core's
 * `NewDraft`, save and autosave, send (with the attachment reminder), mode
 * switching through `ConvertDraft` (with the lossy-conversion warning),
 * attachments and inline images through `UploadAttachment`.
 *
 * Plain-text messages are built exactly like the M1a flows in
 * `state/compose.ts`; those flows stay the fallback while a core does not
 * implement the composer requests yet (`unsupported`).
 */
import { NailError, errorMessageIdOf } from "../../api/client";
import type {
  ComposerMode,
  Draft,
  DraftAttachment,
  DraftId,
  Identity,
  MessageId,
} from "../../api/generated";
import {
  COMPOSER_WINDOW,
  discardComposer,
  startCompose,
  startReply,
  type ComposeContext,
  type ReplyMode,
} from "../../state/compose";
import { queryKeys } from "../../state/queries";
import { bodyOf, draftKey, fromDraft, richDraftOf } from "./draft";
import { needsReminder, parseKeywords } from "./reminder";
import { primeRich, richStoreFor, type RichState } from "./store";

function isUnsupported(error: unknown): boolean {
  return error instanceof NailError && error.code === "unsupported";
}

function fail(ctx: ComposeContext, id: string, args?: Record<string, string | number>): void {
  ctx.store.getState().updateComposer({ busy: null, error: { id, ...(args ? { args } : {}) } });
}

function open(ctx: ComposeContext, draft: Draft, saved: boolean, picked: boolean): void {
  const { composer, rich } = fromDraft(draft, { identityPicked: picked });
  const ui = ctx.store.getState();
  primeRich(ctx.store, rich);
  if (saved) {
    const state = richStoreFor(ctx.store).getState();
    richStoreFor(ctx.store).setState({ savedKey: draftKey(composer, state), autosave: "saved" });
  }
  ui.setComposer(composer);
  ui.openWindow(COMPOSER_WINDOW);
}

/**
 * Opens the composer for a new message from the core's `NewDraft` (the
 * identity's mode and signature). An unfinished message is resumed instead.
 */
export async function startRichCompose(
  ctx: ComposeContext,
  mode: ComposerMode | null = null,
): Promise<void> {
  if (ctx.store.getState().composer) {
    ctx.store.getState().openWindow(COMPOSER_WINDOW);
    return;
  }
  try {
    const { draft } = await ctx.client.request(
      {
        type: "NewDraft",
        identity_id: null,
        account_id: ctx.store.getState().activeAccountId,
        mode,
        context: null,
      },
      "Draft",
    );
    open(ctx, draft, false, false);
  } catch (error) {
    if (!isUnsupported(error)) throw error;
    await startCompose(ctx);
  }
}

/** Opens the composer to reply to or forward a message (`NewDraft` with context). */
export async function startRichReply(
  ctx: ComposeContext,
  messageId: MessageId,
  mode: ReplyMode,
): Promise<void> {
  try {
    const { draft } = await ctx.client.request(
      {
        type: "NewDraft",
        identity_id: null,
        account_id: ctx.store.getState().activeAccountId,
        mode: null,
        context: { mode, message_id: messageId },
      },
      "Draft",
    );
    open(ctx, draft, false, false);
  } catch (error) {
    if (!isUnsupported(error)) throw error;
    await startReply(ctx, messageId, mode);
  }
}

/** Opens a saved draft (any mode) in the composer. */
export async function openRichDraft(ctx: ComposeContext, draftId: DraftId): Promise<void> {
  const { draft } = await ctx.client.request({ type: "GetDraft", draft_id: draftId }, "Draft");
  open(ctx, { ...draft, id: draft.id ?? draftId }, true, true);
}

/** The save in flight, per UI store. */
const saving = new WeakMap<ComposeContext["store"], Promise<DraftId | null>>();

/**
 * Saves the composer as a draft; returns its id, or `null` on failure.
 * `quiet` (autosave) reports only through the autosave state and never
 * blocks the form.
 */
export async function saveRich(
  ctx: ComposeContext,
  options: { quiet?: boolean } = {},
): Promise<DraftId | null> {
  // One save at a time: the first save creates the draft id the next uses.
  for (let busy = saving.get(ctx.store); busy; busy = saving.get(ctx.store)) await busy;
  const run = saveNow(ctx, options.quiet ?? false);
  saving.set(ctx.store, run);
  try {
    return await run;
  } finally {
    saving.delete(ctx.store);
  }
}

async function saveNow(ctx: ComposeContext, quiet: boolean): Promise<DraftId | null> {
  const composer = ctx.store.getState().composer;
  const rich = richStoreFor(ctx.store);
  if (!composer) return null;
  if (composer.identityId === null) {
    if (!quiet) fail(ctx, "composer-error-no-identity");
    return null;
  }
  const state = rich.getState();
  const key = draftKey(composer, state);
  if (quiet) rich.setState({ autosave: "saving" });
  else ctx.store.getState().updateComposer({ busy: "saving", error: null });
  try {
    const { draft_id } = await ctx.client.request(
      { type: "SaveDraft", draft: richDraftOf(composer, state).draft },
      "DraftSaved",
    );
    if (ctx.store.getState().composer) {
      ctx.store
        .getState()
        .updateComposer(quiet ? { draftId: draft_id } : { draftId: draft_id, busy: null });
      rich.setState({ savedKey: key, autosave: "saved" });
    }
    return draft_id;
  } catch (error) {
    if (quiet) rich.setState({ autosave: "failed" });
    else fail(ctx, errorMessageIdOf(error));
    return null;
  }
}

/**
 * Sends the message: now plus the undo-send delay, or at `sendAt`. When the
 * text mentions an attachment but there is none, the reminder asks first
 * (`force` sends anyway).
 */
export async function sendRich(
  ctx: ComposeContext,
  sendAt: string | null,
  options: { force?: boolean } = {},
): Promise<boolean> {
  const composer = ctx.store.getState().composer;
  const rich = richStoreFor(ctx.store);
  if (!composer || composer.busy !== null) return false;
  const state = rich.getState();
  const { draft, invalid } = richDraftOf(composer, state);
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
  if (state.uploads > 0) {
    fail(ctx, "composer-error-uploading");
    return false;
  }
  const keywords = parseKeywords(ctx.t("composer-attachment-keywords"));
  if (
    !options.force &&
    state.reminder !== "dismissed" &&
    needsReminder(
      state.mode,
      bodyOf(composer, state),
      composer.subject,
      state.attachments.length,
      keywords,
    )
  ) {
    rich.setState({ reminder: "confirm" });
    return false;
  }
  const draftId = await saveRich(ctx);
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
export async function discardRich(ctx: ComposeContext): Promise<void> {
  await discardComposer(ctx);
}

function applyConverted(ctx: ComposeContext, mode: ComposerMode, draft: Draft): void {
  const rich = richStoreFor(ctx.store);
  ctx.store.getState().updateComposer({ text: mode === "plain" ? draft.text : "", error: null });
  rich.setState({
    mode,
    html: draft.html ?? "",
    markdown: draft.markdown ?? "",
    markdownPart: draft.markdown_part,
    pendingSwitch: null,
    switching: false,
    htmlSource: false,
  });
}

/**
 * Switches the editing mode. The core converts the body; when that loses
 * formatting the switch waits for `confirmSwitch` / `cancelSwitch`.
 */
export async function switchMode(ctx: ComposeContext, mode: ComposerMode): Promise<void> {
  const composer = ctx.store.getState().composer;
  const rich = richStoreFor(ctx.store);
  const state = rich.getState();
  if (!composer || state.mode === mode || state.switching) return;
  rich.setState({ switching: true, pendingSwitch: null });
  try {
    const { draft, lossy } = await ctx.client.request(
      { type: "ConvertDraft", draft: richDraftOf(composer, state).draft, mode },
      "DraftConverted",
    );
    if (lossy) {
      rich.setState({
        switching: false,
        pendingSwitch: { mode, html: draft.html, markdown: draft.markdown, text: draft.text },
      });
    } else {
      applyConverted(ctx, mode, draft);
    }
  } catch (error) {
    rich.setState({ switching: false });
    fail(ctx, errorMessageIdOf(error));
  }
}

/** Applies a lossy switch the user confirmed. */
export function confirmSwitch(ctx: ComposeContext): void {
  const pending = richStoreFor(ctx.store).getState().pendingSwitch;
  if (!pending) return;
  applyConverted(ctx, pending.mode, {
    text: pending.text,
    html: pending.html,
    markdown: pending.markdown,
    markdown_part: false,
  } as Draft);
}

/** Keeps the current mode. */
export function cancelSwitch(ctx: ComposeContext): void {
  richStoreFor(ctx.store).setState({ pendingSwitch: null });
}

function readAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error("read failed"));
    reader.readAsDataURL(file);
  });
}

/** A fresh `Content-ID` for an inline image. */
export function newContentId(): string {
  const random =
    typeof crypto !== "undefined" && "randomUUID" in crypto
      ? crypto.randomUUID()
      : `${Date.now().toString(36)}${Math.random().toString(36).slice(2)}`;
  return `${random}@nail`;
}

/**
 * Uploads files into the core's blob store and adds them to the draft.
 * `inline` images get a content id (and a local preview for the editor).
 */
export async function uploadFiles(
  ctx: ComposeContext,
  files: readonly File[],
  inline = false,
): Promise<DraftAttachment[]> {
  const rich = richStoreFor(ctx.store);
  const added: DraftAttachment[] = [];
  for (const file of files) {
    rich.setState((s) => ({ uploads: s.uploads + 1 }));
    try {
      const url = await readAsDataUrl(file);
      const data = url.slice(url.indexOf(",") + 1);
      const contentId = inline ? newContentId() : null;
      const { attachment } = await ctx.client.request(
        {
          type: "UploadAttachment",
          filename: file.name,
          mime: file.type === "" ? "application/octet-stream" : file.type,
          data,
          content_id: contentId,
        },
        "AttachmentUploaded",
      );
      added.push(attachment);
      rich.setState((s) => ({
        attachments: [...s.attachments, attachment],
        reminder: null,
        inlinePreviews:
          attachment.content_id !== null
            ? { ...s.inlinePreviews, [attachment.content_id]: url }
            : s.inlinePreviews,
      }));
    } catch (error) {
      fail(ctx, errorMessageIdOf(error));
    } finally {
      rich.setState((s) => ({ uploads: s.uploads - 1 }));
    }
  }
  return added;
}

/** Removes an attachment from the draft. */
export function removeAttachment(ctx: ComposeContext, index: number): void {
  richStoreFor(ctx.store).setState((s) => ({
    attachments: s.attachments.filter((_, i) => i !== index),
  }));
}

function signatureText(identity: Identity | undefined, mode: ComposerMode): string | null {
  const sig = identity?.signature;
  if (!sig) return null;
  const text = mode === "markdown" ? (sig.markdown ?? sig.text) : sig.text;
  return text && text.trim() !== "" ? `-- \n${text.trimEnd()}` : null;
}

/**
 * Swaps the signature block when the identity changes (plain text and
 * Markdown: the previous identity's block is replaced where it stands).
 */
export function swapSignature(
  ctx: ComposeContext,
  from: Identity | undefined,
  to: Identity | undefined,
): void {
  const composer = ctx.store.getState().composer;
  const rich = richStoreFor(ctx.store);
  const state: RichState = rich.getState();
  if (!composer || state.signature === "none" || state.mode === "html") return;
  const previous = signatureText(from, state.mode);
  const next = signatureText(to, state.mode);
  const body = bodyOf(composer, state);
  if (previous === null || previous === next) return;
  const at = body.lastIndexOf(previous);
  if (at < 0) return;
  const swapped = body.slice(0, at) + (next ?? "") + body.slice(at + previous.length);
  if (state.mode === "plain") ctx.store.getState().updateComposer({ text: swapped });
  else rich.setState({ markdown: swapped });
}
