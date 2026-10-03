/**
 * Rich composer flows: new message / reply / forward through the core's
 * `NewDraft`, save and autosave, send (with the attachment reminder), mode
 * switching through `ConvertDraft` (with the lossy-conversion warning),
 * attachments and inline images through `UploadAttachment`.
 *
 * Plain-text messages are built exactly like the M1a flows in
 * `state/compose.ts`; those flows stay the fallback while a core does not
 * implement the composer requests yet (`unsupported`).
 *
 * M5c (WP172): every new message, reply, forward or reopened draft opens
 * its own composer window `composer:<key>` (`./sessions.ts`); a draft that
 * is open already is focused instead. The flows take the composer window
 * as an option; without one they work on the focused composer (the
 * actions of toolbars, menus and the palette), else the plain `composer`.
 */
import { NailError, errorMessageIdOf } from "../../api/client";
import type {
  ComposerMode,
  Draft,
  DraftAttachment,
  DraftId,
  Identity,
  MailtoLink,
  MessageId,
} from "../../api/generated";
import {
  cancelSend,
  startCompose,
  startReply,
  type ComposeContext,
  type ReplyMode,
} from "../../state/compose";
import { queryKeys } from "../../state/queries";
import { createUiStore, type ComposerState } from "../../state/uiStore";
import { sendDraftChecked } from "../plugins/SendVeto";
import { composerSendBlock, composerSendCrypto } from "../security";
import { bodyOf, draftKey, fromDraft, richDraftOf } from "./draft";
import { needsReminder, parseKeywords } from "./reminder";
import { loadInlinePreviews } from "./inlineImages";
import {
  CLOSE_CONFIRM_KIND,
  composerOf,
  composerSessions,
  composerTarget,
  confirmWindowFor,
  cryptoFor,
  DISCARD_CONFIRM_KIND,
  findComposerByDraft,
  isUnsaved,
  markClosing,
  newComposerWindow,
  patchComposer,
  setComposerFor,
  setCryptoFor,
} from "./sessions";
import {
  cachedPreviews,
  COMPOSER_KIND,
  dropRichStore,
  emptyRich,
  isComposerInstance,
  richStoreFor,
  type RichState,
} from "./store";

/** Which composer a flow works on (default: the focused one). */
export interface ComposerTargetOption {
  /** The composer window (`composer:<key>` or `composer`). */
  window?: string | null;
}

/**
 * Whether `NewDraft` failed in a way the plain-text (M1a) flow handles: the
 * core has no composer, or it cannot pick an identity (e.g. a demo account
 * without one; the plain composer then asks for one before sending).
 */
function fallsBackToPlain(error: unknown): boolean {
  return (
    error instanceof NailError && (error.code === "unsupported" || error.code === "invalid_request")
  );
}

function fail(
  ctx: ComposeContext,
  window: string,
  id: string,
  args?: Record<string, string | number>,
): void {
  patchComposer(ctx.store, window, { busy: null, error: { id, ...(args ? { args } : {}) } });
}

/**
 * Opens a composer window for a message: a new `composer:<key>` window on
 * top (the window manager places it, cascaded from the previous one), or
 * the open window of the same draft. Returns the window id.
 */
export function openComposer(
  ctx: Pick<ComposeContext, "store">,
  composer: ComposerState,
  rich: Partial<RichState> = {},
  options: { saved?: boolean; crypto?: Draft["crypto"] | null; window?: string | null } = {},
): string {
  composerSessions(ctx.store);
  const ui = ctx.store.getState();
  const open = composer.draftId === null ? null : findComposerByDraft(ui, composer.draftId);
  if (open !== null) {
    ui.openWindow(open);
    return open;
  }
  // A given window id (a pop-out loading `#/window/composer:<key>`) when no
  // composer has it; the pop-out page has the window open but empty.
  const wanted = options.window;
  const free = isComposerInstance(wanted) && !Object.hasOwn(ui.composers, wanted);
  const window = free ? wanted : newComposerWindow(ui, composer.draftId);
  const store = richStoreFor(ctx.store, window);
  store.setState({ ...emptyRich(), inlinePreviews: cachedPreviews(ctx.store), ...rich });
  // The saved sign / encrypt choice (a reply to encrypted mail starts
  // encrypted); it applies once the window is focused, i.e. right away.
  const crypto = options.crypto ?? null;
  setCryptoFor(ctx.store, window, crypto);
  if (options.saved) {
    store.setState({ savedKey: draftKey(composer, store.getState(), crypto), autosave: "saved" });
  }
  ui.setComposerOf(window, composer);
  ui.openWindow(window);
  return window;
}

function open(
  ctx: ComposeContext,
  draft: Draft,
  saved: boolean,
  picked: boolean,
  window: string | null = null,
): string {
  const { composer, rich } = fromDraft(draft, { identityPicked: picked });
  return openComposer(ctx, composer, rich, { saved, crypto: draft.crypto ?? null, window });
}

/**
 * Runs an M1a flow of `state/compose.ts` (the fallback) on a scratch UI
 * store and opens what it composed in a window of its own, so it never
 * resumes or overwrites a composer that is open already.
 */
async function openLegacy(
  ctx: ComposeContext,
  run: (scratch: ComposeContext) => Promise<void>,
): Promise<string | null> {
  const scratch = createUiStore({ activeAccountId: ctx.store.getState().activeAccountId });
  await run({ ...ctx, store: scratch });
  const composer = scratch.getState().composer;
  return composer === null ? null : openComposer(ctx, composer);
}

/** Closes a composer a flow has finished with (sent, discarded). */
function finish(ctx: Pick<ComposeContext, "store">, window: string): void {
  const ui = ctx.store.getState();
  if (isComposerInstance(window)) {
    markClosing(ctx.store, window);
    ui.closeWindow(window);
    setComposerFor(ctx.store, window, null);
    dropRichStore(ctx.store, window);
  } else {
    ui.setComposer(null);
    ui.closeWindow(COMPOSER_KIND);
  }
}

/**
 * Opens a new composer window for a new message from the core's `NewDraft`
 * (the identity's mode and signature). Composers that are open already stay
 * open (M5c: one window per draft).
 */
export async function startRichCompose(
  ctx: ComposeContext,
  mode: ComposerMode | null = null,
): Promise<void> {
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
    if (!fallsBackToPlain(error)) throw error;
    await openLegacy(ctx, startCompose);
  }
}

/**
 * Opens a composer for a `mailto:` link (a launch request): a new plain
 * text draft with the link's recipients, subject and body above the
 * signature, in a window of its own (never merged into another draft).
 */
export async function startMailtoCompose(ctx: ComposeContext, link: MailtoLink): Promise<void> {
  const addresses = (list: readonly string[]) => list.map((email) => ({ name: null, email }));
  try {
    const { draft } = await ctx.client.request(
      {
        type: "NewDraft",
        identity_id: null,
        account_id: ctx.store.getState().activeAccountId,
        mode: "plain",
        context: null,
      },
      "Draft",
    );
    const body = link.body ?? "";
    open(
      ctx,
      {
        ...draft,
        to: addresses(link.to),
        cc: addresses(link.cc),
        bcc: addresses(link.bcc),
        subject: link.subject ?? draft.subject,
        text: body === "" ? draft.text : `${body}\n\n${draft.text}`.trimEnd(),
      },
      false,
      false,
    );
  } catch (error) {
    if (!fallsBackToPlain(error)) throw error;
    const window = await openLegacy(ctx, startCompose);
    if (window === null) return;
    const text = composerOf(ctx.store.getState(), window)?.text ?? "";
    patchComposer(ctx.store, window, {
      to: link.to.join(", "),
      cc: link.cc.join(", "),
      bcc: link.bcc.join(", "),
      showCc: link.cc.length > 0,
      showBcc: link.bcc.length > 0,
      subject: link.subject ?? "",
      text: link.body ? `${link.body}\n\n${text}`.trimEnd() : text,
    });
  }
}

/** Opens a composer to reply to or forward a message (`NewDraft` with context). */
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
    if (!fallsBackToPlain(error)) throw error;
    await openLegacy(ctx, (scratch) => startReply(scratch, messageId, mode));
  }
}

/**
 * Opens a saved draft (any mode) in a composer window; a draft that is open
 * already is focused instead. `options.window` names the window to use when
 * it is free (a desktop pop-out shows the draft of the composer it was
 * popped out from: `#/window/composer:<key>` with the context `{ draft }`).
 */
export async function openRichDraft(
  ctx: ComposeContext,
  draftId: DraftId,
  options: ComposerTargetOption = {},
): Promise<void> {
  const already = findComposerByDraft(ctx.store.getState(), draftId);
  if (already !== null) {
    ctx.store.getState().openWindow(already);
    return;
  }
  const { draft } = await ctx.client.request({ type: "GetDraft", draft_id: draftId }, "Draft");
  const opened = { ...draft, id: draft.id ?? draftId };
  const window = open(ctx, opened, true, true, options.window ?? null);
  // Inline images saved in another session: fetch their previews.
  if (opened.mode === "html") void loadInlinePreviews(ctx, opened, window);
}

/** The saves in flight, per UI store and composer window. */
const saving = new WeakMap<ComposeContext["store"], Map<string, Promise<DraftId | null>>>();

function savesOf(store: ComposeContext["store"]): Map<string, Promise<DraftId | null>> {
  let map = saving.get(store);
  if (!map) {
    map = new Map();
    saving.set(store, map);
  }
  return map;
}

/**
 * Saves the composer as a draft; returns its id, or `null` on failure.
 * `quiet` (autosave) reports only through the autosave state and never
 * blocks the form.
 */
export async function saveRich(
  ctx: ComposeContext,
  options: { quiet?: boolean } & ComposerTargetOption = {},
): Promise<DraftId | null> {
  const window = composerTarget(ctx.store.getState(), options.window);
  const saves = savesOf(ctx.store);
  // One save at a time: the first save creates the draft id the next uses.
  for (let busy = saves.get(window); busy; busy = saves.get(window)) await busy;
  const run = saveNow(ctx, window, options.quiet ?? false);
  saves.set(window, run);
  try {
    return await run;
  } finally {
    saves.delete(window);
  }
}

async function saveNow(
  ctx: ComposeContext,
  window: string,
  quiet: boolean,
): Promise<DraftId | null> {
  const composer = composerOf(ctx.store.getState(), window);
  const rich = richStoreFor(ctx.store, window);
  if (!composer) return null;
  if (composer.identityId === null) {
    if (!quiet) fail(ctx, window, "composer-error-no-identity");
    return null;
  }
  const state = rich.getState();
  const crypto = cryptoFor(ctx.store, window);
  const key = draftKey(composer, state, crypto);
  if (quiet) rich.setState({ autosave: "saving" });
  else patchComposer(ctx.store, window, { busy: "saving", error: null });
  try {
    const { draft_id } = await ctx.client.request(
      { type: "SaveDraft", draft: richDraftOf(composer, state, crypto).draft },
      "DraftSaved",
    );
    if (composerOf(ctx.store.getState(), window)) {
      patchComposer(
        ctx.store,
        window,
        quiet ? { draftId: draft_id } : { draftId: draft_id, busy: null },
      );
      rich.setState({ savedKey: key, autosave: "saved" });
    }
    void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.drafts });
    return draft_id;
  } catch (error) {
    if (quiet) rich.setState({ autosave: "failed" });
    else fail(ctx, window, errorMessageIdOf(error));
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
  options: { force?: boolean } & ComposerTargetOption = {},
): Promise<boolean> {
  const window = composerTarget(ctx.store.getState(), options.window);
  const composer = composerOf(ctx.store.getState(), window);
  const rich = richStoreFor(ctx.store, window);
  if (!composer || composer.busy !== null) return false;
  // The sign / encrypt toggles and the send guard are the focused
  // composer's: sending from a composer focuses it.
  if (isComposerInstance(window)) ctx.store.getState().focusWindow(window);
  const state = rich.getState();
  const { draft, invalid } = richDraftOf(composer, state);
  if (invalid.length > 0) {
    fail(ctx, window, "composer-error-invalid-address", { addresses: invalid.join(", ") });
    return false;
  }
  if (draft.to.length + draft.cc.length + draft.bcc.length === 0) {
    fail(ctx, window, "composer-error-no-recipients");
    return false;
  }
  if (sendAt !== null && Number.isNaN(Date.parse(sendAt))) {
    fail(ctx, window, "composer-error-send-at");
    return false;
  }
  if (state.uploads > 0) {
    fail(ctx, window, "composer-error-uploading");
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
  // A chosen protection that cannot work is explained, never dropped. While
  // the key check is still running (or no key panel is shown) the core
  // decides: it refuses with `no_key` / `locked` rather than send plain.
  const block = composerSendBlock();
  if (block && block.id !== "security-compose-block-checking") {
    fail(ctx, window, block.id, {
      count: block.recipients.length,
      recipients: block.recipients.join(", "),
    });
    return false;
  }
  const draftId = await saveRich(ctx, { window });
  if (draftId === null) return false;
  patchComposer(ctx.store, window, { busy: "sending" });
  const crypto = composerSendCrypto();
  try {
    // M4a: a plug-in's before-send hook may veto; the veto dialog decides.
    const entry = await sendDraftChecked(ctx.client, ctx.store, {
      type: "SendDraft",
      draft_id: draftId,
      delay_secs: null,
      send_at: sendAt,
      ...(crypto ? { crypto } : {}),
    });
    if (entry === null) {
      // "Keep editing": the composer stays open.
      patchComposer(ctx.store, window, { busy: null });
      return false;
    }
    ctx.store.getState().setLastSend(entry);
    finish(ctx, window);
    void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.outbox });
    void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.drafts });
    return true;
  } catch (error) {
    fail(ctx, window, errorMessageIdOf(error));
    return false;
  }
}

/** Throws the message away (and its saved draft) without asking. */
export async function discardRich(ctx: ComposeContext, window?: string | null): Promise<void> {
  const target = composerTarget(ctx.store.getState(), window);
  const composer = composerOf(ctx.store.getState(), target);
  finish(ctx, target);
  if (composer?.draftId) {
    try {
      await ctx.client.request({ type: "DeleteDraft", draft_id: composer.draftId }, "Done");
    } catch (error) {
      if (!(error instanceof NailError && error.code === "not_found")) throw error;
    }
    void ctx.queryClient.invalidateQueries({ queryKey: queryKeys.drafts });
  }
}

/**
 * The composer's Discard button: asks first (the modal confirmation
 * `composer-discard`) when there is something to lose — a saved draft or
 * unsaved changes; an untouched new message goes at once.
 */
export async function requestDiscard(ctx: ComposeContext, window?: string | null): Promise<void> {
  const target = composerTarget(ctx.store.getState(), window);
  const composer = composerOf(ctx.store.getState(), target);
  if (!composer) return;
  if (composer.draftId === null && !isUnsaved(ctx.store, target)) {
    await discardRich(ctx, target);
    return;
  }
  ctx.store.getState().openWindow(confirmWindowFor(DISCARD_CONFIRM_KIND, target));
}

/**
 * Closes a composer window: at once when everything is saved, else after
 * the modal confirmation `composer-close` (save, close without saving,
 * keep editing). Closing keeps the saved draft (Drafts in the outbox).
 */
export function requestClose(ctx: Pick<ComposeContext, "store">, window: string): void {
  if (isComposerInstance(window) && isUnsaved(ctx.store, window)) {
    ctx.store.getState().openWindow(confirmWindowFor(CLOSE_CONFIRM_KIND, window));
  } else {
    closeComposerNow(ctx, window);
  }
}

/** Closes a composer window without asking (its last saved draft stays). */
export function closeComposerNow(ctx: Pick<ComposeContext, "store">, window: string): void {
  if (isComposerInstance(window)) markClosing(ctx.store, window);
  ctx.store.getState().closeWindow(window);
}

function applyConverted(
  ctx: ComposeContext,
  window: string,
  mode: ComposerMode,
  draft: Draft,
): void {
  const rich = richStoreFor(ctx.store, window);
  patchComposer(ctx.store, window, { text: mode === "plain" ? draft.text : "", error: null });
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
export async function switchMode(
  ctx: ComposeContext,
  mode: ComposerMode,
  window?: string | null,
): Promise<void> {
  const target = composerTarget(ctx.store.getState(), window);
  const composer = composerOf(ctx.store.getState(), target);
  const rich = richStoreFor(ctx.store, target);
  const state = rich.getState();
  if (!composer || state.mode === mode || state.switching) return;
  rich.setState({ switching: true, pendingSwitch: null });
  try {
    const { draft, lossy } = await ctx.client.request(
      {
        type: "ConvertDraft",
        draft: richDraftOf(composer, state, cryptoFor(ctx.store, target)).draft,
        mode,
      },
      "DraftConverted",
    );
    if (lossy) {
      rich.setState({
        switching: false,
        pendingSwitch: { mode, html: draft.html, markdown: draft.markdown, text: draft.text },
      });
    } else {
      applyConverted(ctx, target, mode, draft);
    }
  } catch (error) {
    rich.setState({ switching: false });
    fail(ctx, target, errorMessageIdOf(error));
  }
}

/** Applies a lossy switch the user confirmed. */
export function confirmSwitch(ctx: ComposeContext, window?: string | null): void {
  const target = composerTarget(ctx.store.getState(), window);
  const pending = richStoreFor(ctx.store, target).getState().pendingSwitch;
  if (!pending) return;
  applyConverted(ctx, target, pending.mode, {
    text: pending.text,
    html: pending.html,
    markdown: pending.markdown,
    markdown_part: false,
  } as Draft);
}

/** Keeps the current mode. */
export function cancelSwitch(ctx: ComposeContext, window?: string | null): void {
  richStoreFor(ctx.store, composerTarget(ctx.store.getState(), window)).setState({
    pendingSwitch: null,
  });
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
  window?: string | null,
): Promise<DraftAttachment[]> {
  const target = composerTarget(ctx.store.getState(), window);
  const rich = richStoreFor(ctx.store, target);
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
      fail(ctx, target, errorMessageIdOf(error));
    } finally {
      rich.setState((s) => ({ uploads: s.uploads - 1 }));
    }
  }
  return added;
}

/** Removes an attachment from the draft. */
export function removeAttachment(ctx: ComposeContext, index: number, window?: string | null): void {
  richStoreFor(ctx.store, composerTarget(ctx.store.getState(), window)).setState((s) => ({
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
  window?: string | null,
): void {
  const target = composerTarget(ctx.store.getState(), window);
  const composer = composerOf(ctx.store.getState(), target);
  const rich = richStoreFor(ctx.store, target);
  const state: RichState = rich.getState();
  if (!composer || state.signature === "none" || state.mode === "html") return;
  const previous = signatureText(from, state.mode);
  const next = signatureText(to, state.mode);
  const body = bodyOf(composer, state);
  if (previous === null || previous === next) return;
  const at = body.lastIndexOf(previous);
  if (at < 0) return;
  const swapped = body.slice(0, at) + (next ?? "") + body.slice(at + previous.length);
  if (state.mode === "plain") patchComposer(ctx.store, target, { text: swapped });
  else rich.setState({ markdown: swapped });
}

/** Undo send: the entry goes back to being a draft, reopened in a composer window. */
export async function cancelRichSend(ctx: ComposeContext, outboxId: string): Promise<void> {
  await cancelSend(ctx, outboxId, openRichDraft);
}
