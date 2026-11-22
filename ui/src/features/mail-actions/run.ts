/**
 * Runs a mail action on its targets (P3, ADR 0164): one `ApplyMailAction`,
 * the list and the reader updated at once, the selection moved on like
 * Gmail (to the next conversation after Archive / Delete, to the next card
 * after a card's own Delete), and the undo toast.
 *
 * R5 (ADR 0235): always one request, whatever the size — the core runs a
 * large action in parts as one batch with one undo. A whole list ("select
 * all") is one list target; it asks first with the true count
 * (`./bulk.ts`), and the list updates its loaded rows at once.
 */
import { NailError, errorMessageOf } from "../../api/client";
import type {
  FolderId,
  FolderRole,
  MailAction,
  MailActionResult,
  MailBatchId,
  MailTargetResolution,
  MessageId,
  MessageTarget,
  Response,
  ThreadId,
} from "../../api/generated";
import {
  hasListTarget,
  listSelectionFor,
  loadedRowTargets,
  messageIdsOf,
  messageTarget,
  threadIdsOf,
  withAsOf,
  type ActionTargets,
  type ListScope,
} from "../../actions/mailTargets";
import {
  cachedFolder,
  findSummary,
  findThread,
  nextSelection,
  patchSummaries,
  patchThreads,
  refreshAfterAction,
  type WriteContext,
} from "../../state/mailOps";
import { queryKeys } from "../../state/queries";
import type { UiStore } from "../../state/uiStore";
import { confirmBulk } from "./bulk";
import { mailActionsStoreFor, type SelectionBefore, type TagToast } from "./store";

type ConversationResponse = Extract<Response, { type: "Conversation" }>;

/** The role of a cached folder (`null` when unknown). */
export function folderRole(ctx: WriteContext, folderId: FolderId | null): FolderRole | null {
  if (folderId === null) return null;
  return cachedFolder(ctx.queryClient, folderId)?.role ?? null;
}

/**
 * Whether a message is a draft to continue writing rather than to read:
 * flagged `\Draft` (`false` when unknown). Being in a Drafts folder is not
 * enough (R4, ADR 0230): received mail moved there opens in the reader, so
 * continuing it can never replace or delete it unasked.
 */
export function isDraftMessage(ctx: WriteContext, id: MessageId | null): boolean {
  if (id === null) return false;
  return findSummary(ctx.queryClient, id)?.flags.draft === true;
}

/**
 * Whether a message sits in a Drafts folder without being a draft (no
 * `\Draft` flag), e.g. received mail moved there by mistake: the reader
 * offers to move it out (`mail.move-out-of-drafts`).
 */
export function isMisplacedInDrafts(ctx: WriteContext, id: MessageId | null): boolean {
  if (id === null) return false;
  const summary = findSummary(ctx.queryClient, id);
  if (summary === undefined || summary.flags.draft) return false;
  return folderRole(ctx, summary.folder_id) === "drafts";
}

/**
 * Whether an action takes its rows out of the list shown: moves out of the
 * listed folder do, flags and tags do not. While searching only Trash and
 * Delete permanently do (hits stay hits wherever they are).
 */
export function removesRows(
  action: MailAction,
  listedRole: FolderRole | null,
  searching: boolean,
): boolean {
  switch (action.type) {
    case "mark-read":
    case "flag":
    case "tags":
      return false;
    case "trash":
    case "delete-permanently":
      return true;
    case "archive":
      return !searching && listedRole !== "archive";
    case "move":
    case "snooze":
      return !searching;
    case "junk":
      return !searching && action.junk !== (listedRole === "junk");
  }
}

/** The list rows (message ids of `messageOrder`) the targets stand for. */
export function rowsOf(ui: UiStore, targets: readonly MessageTarget[]): MessageId[] {
  const { messageOrder, messageThreads } = ui.getState();
  const threads = new Set<ThreadId>(threadIdsOf(targets));
  const messages = new Set<MessageId>(messageIdsOf(targets));
  return messageOrder.filter((id) => {
    const thread = messageThreads[id];
    return messages.has(id) || (thread !== undefined && threads.has(thread));
  });
}

/** The row of the selection: the selected message's own row, else its conversation's. */
function selectedRow(ui: UiStore): MessageId | null {
  const { messageOrder, messageThreads, selectedMessageId, selectedThreadId } = ui.getState();
  if (selectedMessageId === null) return null;
  if (messageOrder.includes(selectedMessageId)) return selectedMessageId;
  return messageOrder.find((id) => messageThreads[id] === selectedThreadId) ?? null;
}

/** Takes rows out of the list and selects the next row, like Gmail. */
function removeRows(ctx: WriteContext, targets: readonly MessageTarget[]): void {
  const ui = ctx.store.getState();
  const removed = rowsOf(ctx.store, targets);
  const current = selectedRow(ctx.store);
  const next = nextSelection(ui.messageOrder, current, removed);
  patchThreads(ctx.queryClient, threadIdsOf(targets), () => null);
  patchSummaries(ctx.queryClient, [...removed, ...messageIdsOf(targets)], () => null);
  const gone = new Set(removed);
  ui.setMessageOrder(
    ui.messageOrder.filter((id) => !gone.has(id)),
    ui.messageThreads,
  );
  listSelectionFor(ctx.store).getState().setPicked([]);
  if (next !== current || (current !== null && gone.has(current))) {
    ui.selectMessage(next);
    ui.selectThread(next === null ? null : (ui.messageThreads[next] ?? null));
  }
}

/** The cached conversation of a thread, as the reader shows it. */
function cachedConversation(ctx: WriteContext, threadId: ThreadId): ConversationResponse | null {
  for (const [, data] of ctx.queryClient.getQueriesData<ConversationResponse>({
    queryKey: queryKeys.thread(threadId),
  })) {
    if (data && "thread" in data && Array.isArray(data.messages)) return data;
  }
  return null;
}

/**
 * Takes a card's message out (its own Delete / Move): the reader selects
 * the next card of the conversation, else the one before; when none is
 * left, the conversation's row leaves the list too.
 */
function removeCards(ctx: WriteContext, ids: readonly MessageId[]): void {
  const ui = ctx.store.getState();
  const threadId = ui.selectedThreadId;
  const conversation = threadId === null ? null : cachedConversation(ctx, threadId);
  const gone = new Set(ids);
  const order = conversation?.messages.map((m) => m.id) ?? [];
  const left = order.filter((id) => !gone.has(id));
  if (threadId !== null && conversation !== null && left.length === 0) {
    removeRows(ctx, [{ kind: "conversation", thread_id: threadId, folder_id: null }]);
    return;
  }
  const next =
    ui.selectedMessageId === null ? null : nextSelection(order, ui.selectedMessageId, ids);
  patchSummaries(ctx.queryClient, ids, () => null);
  if (next !== ui.selectedMessageId) ui.selectMessage(next);
}

/** Mirrors an action's flags and tags in the caches at once. */
function patchFlags(ctx: WriteContext, targets: readonly MessageTarget[], action: MailAction) {
  const threads = threadIdsOf(targets);
  const ids = messageIdsOf(targets);
  if (action.type === "mark-read") {
    patchThreads(ctx.queryClient, threads, (t) => ({
      ...t,
      unread_count: action.read ? 0 : t.message_count,
      latest_message: {
        ...t.latest_message,
        flags: { ...t.latest_message.flags, seen: action.read },
      },
    }));
    patchMessages(ctx, ids, action);
  } else if (action.type === "flag") {
    patchThreads(ctx.queryClient, threads, (t) => ({ ...t, flagged: action.flagged }));
    patchMessages(ctx, ids, action);
  }
}

/** Patches the messages an action changed (flags and tags). */
function patchMessages(ctx: WriteContext, ids: readonly MessageId[], action: MailAction) {
  if (ids.length === 0) return;
  switch (action.type) {
    case "mark-read":
      patchSummaries(ctx.queryClient, ids, (m) => ({
        ...m,
        flags: { ...m.flags, seen: action.read },
      }));
      break;
    case "flag":
      patchSummaries(ctx.queryClient, ids, (m) => ({
        ...m,
        flags: { ...m.flags, flagged: action.flagged },
      }));
      break;
    case "tags":
      patchSummaries(ctx.queryClient, ids, (m) => ({
        ...m,
        tags: [...m.tags.filter((tag) => !action.remove.includes(tag)), ...action.add].filter(
          (tag, index, all) => all.indexOf(tag) === index,
        ),
      }));
      break;
    default:
      break;
  }
}

function selectionBefore(ui: UiStore): SelectionBefore {
  const { selectedMessageId, selectedThreadId } = ui.getState();
  return { messageId: selectedMessageId, threadId: selectedThreadId };
}

/** Shows an error of the core in the toast (its own text when it sent one). */
function showError(ctx: WriteContext, error: unknown): void {
  mailActionsStoreFor(ctx.store)
    .getState()
    .show({ kind: "error", message: errorMessageOf(error) });
  if (!(error instanceof NailError)) console.error("nail: mail action failed", error);
}

/** Options of {@link runMailAction}. */
export interface RunOptions {
  /**
   * The confirmation that counted the targets (`ResolveMailTargets`): the
   * core acts on exactly what it counted — its messages, or a list target
   * with its `as_of` mark — so nothing that arrived meanwhile is acted on
   * unasked. A whole list without one asks first (`./bulk.ts`).
   */
  resolution?: MailTargetResolution;
  /** What a tag change calls its tags in the toast (Gmail labels, Exchange categories). */
  tagNoun?: TagNoun;
}

/**
 * The options that act on exactly the messages a confirmation counted
 * (`ResolveMailTargets`: Delete permanently, a move into Drafts, Trash or
 * Junk).
 */
export function runOptionsOf(resolution: MailTargetResolution): RunOptions {
  return { resolution };
}

/**
 * What the core acts on after a confirmation: a whole list with the
 * confirmation's mark, else exactly the messages it counted.
 */
export function confirmedTargets(
  targets: readonly MessageTarget[],
  resolution: MailTargetResolution,
): MessageTarget[] {
  return hasListTarget(targets)
    ? withAsOf(targets, resolution.as_of)
    : resolution.message_ids.map(messageTarget);
}

/** The targets with each list target replaced by its loaded rows (the list's own update). */
function loadedTargets(
  ui: UiStore,
  targets: readonly MessageTarget[],
  scope: ListScope,
): MessageTarget[] {
  if (!hasListTarget(targets)) return [...targets];
  const { messageOrder, messageThreads } = ui.getState();
  return targets.flatMap((t) => loadedRowTargets(t, messageOrder, messageThreads, scope));
}

/** Resolves a whole list and asks before acting on it; `null` = do not act. */
async function confirmList(
  ctx: WriteContext,
  targets: readonly MessageTarget[],
  action: MailAction,
): Promise<MailTargetResolution | null> {
  let resolution: MailTargetResolution;
  try {
    ({ resolution } = await ctx.client.request(
      { type: "ResolveMailTargets", targets: [...targets], action },
      "MailTargets",
    ));
  } catch (error) {
    showError(ctx, error);
    return null;
  }
  if (resolution.messages === 0) return null;
  return (await confirmBulk(ctx.store, action, resolution)) ? resolution : null;
}

/** What the toast calls the tags of a `tags` action. */
export type TagNoun = "label" | "category" | "tag";

/**
 * Applies `action` to `targets` through the core (one batch) and reports
 * it in the undo toast. Returns the core's answer, `null` on failure (the
 * toast shows why, the caches are refetched).
 */
export async function runMailAction(
  ctx: WriteContext,
  { targets, source }: ActionTargets,
  action: MailAction,
  options: RunOptions = {},
): Promise<MailActionResult | null> {
  if (targets.length === 0) return null;
  let resolution = options.resolution;
  if (resolution === undefined && hasListTarget(targets)) {
    const confirmed = await confirmList(ctx, targets, action);
    if (confirmed === null) return null;
    resolution = confirmed;
  }
  const ui = ctx.store.getState();
  const scope = listSelectionFor(ctx.store).getState().scope;
  // The list updates the rows it has loaded; the core acts on the whole list.
  const shown = loadedTargets(ctx.store, targets, scope);
  const searching = ui.search !== null;
  const listed = scope.folderId ?? (searching ? null : ui.activeFolderId);
  const selection = selectionBefore(ctx.store);
  if (removesRows(action, folderRole(ctx, listed), searching)) {
    if (source === "card") removeCards(ctx, messageIdsOf(shown));
    else removeRows(ctx, shown);
  } else {
    patchFlags(ctx, shown, action);
  }
  let result: MailActionResult;
  try {
    const apply = resolution === undefined ? [...targets] : confirmedTargets(targets, resolution);
    ({ result } = await ctx.client.request(
      { type: "ApplyMailAction", targets: apply, action },
      "MailActionQueued",
    ));
    // The toast names the conversations the confirmation named.
    if (resolution !== undefined && !hasListTarget(targets)) {
      result = { ...result, conversations: resolution.conversations };
    }
  } catch (error) {
    showError(ctx, error);
    void ctx.queryClient.invalidateQueries();
    return null;
  }
  ctx.store.getState().opQueued(result.op_ids);
  patchMessages(ctx, result.message_ids, action);
  refreshAfterAction(ctx.queryClient);
  const store = mailActionsStoreFor(ctx.store).getState();
  store.show({
    kind: "done",
    action: result.kind,
    conversations: result.conversations,
    messages: result.message_ids.length,
    batchId: result.batch_id,
    tags: action.type === "tags" ? tagToast(action.add, action.remove, options.tagNoun) : undefined,
    leftOut: result.left_out,
  });
  if (result.batch_id !== null) {
    const until = result.undo_until === null ? NaN : Date.parse(result.undo_until);
    store.setLast({
      batchId: result.batch_id,
      until: Number.isNaN(until) ? Number.POSITIVE_INFINITY : until,
      selection: source === "list" ? selection : null,
    });
  }
  return result;
}

/** What the toast says about a tag change. */
export function tagToast(
  add: readonly string[],
  remove: readonly string[],
  noun: TagNoun = "tag",
): TagToast {
  const change = remove.length === 0 ? "added" : add.length === 0 ? "removed" : "changed";
  return { change, noun, count: add.length + remove.length };
}

/** Whether `mail.undo` has a batch to undo. */
export function canUndo(ui: UiStore, now = Date.now()): boolean {
  const last = mailActionsStoreFor(ui).getState().last;
  return last !== null && last.until > now;
}

/**
 * Undoes the last batch (`mail.undo`, the toast's Undo): every message
 * back where it was; the selection goes back to what it was before.
 */
export async function undoLast(ctx: WriteContext, batchId?: MailBatchId): Promise<void> {
  const store = mailActionsStoreFor(ctx.store);
  const remembered = store.getState().last;
  // The toast's Undo names its batch; `mail.undo` takes the last one.
  const last =
    batchId === undefined || remembered?.batchId === batchId
      ? remembered
      : { batchId, until: Number.POSITIVE_INFINITY, selection: null };
  if (last === null) return;
  if (last === remembered) store.getState().setLast(null);
  try {
    const { op_ids } = await ctx.client.request(
      { type: "UndoMailAction", batch_id: last.batchId },
      "Queued",
    );
    ctx.store.getState().opQueued(op_ids);
  } catch (error) {
    showError(ctx, error);
    return;
  } finally {
    refreshAfterAction(ctx.queryClient);
  }
  store.getState().show({ kind: "undone" });
  if (last.selection !== null && last.selection.messageId !== null) {
    const ui = ctx.store.getState();
    ui.selectMessage(last.selection.messageId);
    ui.selectThread(last.selection.threadId);
  }
}

/** A whole list's folder has unread mail; `null` = unknown (a search, a folder not cached). */
function listFolderUnread(ctx: WriteContext, target: MessageTarget): boolean | null {
  if (target.kind !== "list" || target.list.source === "search") return null;
  const folder = cachedFolder(ctx.queryClient, target.list.folder_id);
  return folder === undefined ? null : folder.unread > 0;
}

/** The targets with a whole list replaced by its loaded rows (what the UI knows of it). */
function knownTargets(ctx: WriteContext, targets: readonly MessageTarget[]): MessageTarget[] {
  return loadedTargets(ctx.store, targets, listSelectionFor(ctx.store).getState().scope);
}

/**
 * Whether any of the targets has unread mail (toggle read = mark read
 * then). A whole list asks its folder's unread count, else its loaded rows.
 */
export function anyUnread(ctx: WriteContext, targets: readonly MessageTarget[]): boolean {
  const lists = targets.map((t) => listFolderUnread(ctx, t));
  if (lists.includes(true)) return true;
  return knownTargets(ctx, targets).some((target) => {
    if (target.kind === "conversation") {
      const thread = findThread(ctx.queryClient, target.thread_id);
      if (thread) return thread.unread_count > 0;
      const selected = ctx.store.getState().selectedMessageId;
      return selected !== null && findSummary(ctx.queryClient, selected)?.flags.seen === false;
    }
    return (
      target.kind === "message" &&
      findSummary(ctx.queryClient, target.message_id)?.flags.seen === false
    );
  });
}

/** Whether every target is flagged (toggle flag = unflag then); a whole list by its loaded rows. */
export function allFlagged(ctx: WriteContext, targets: readonly MessageTarget[]): boolean {
  const known = knownTargets(ctx, targets);
  return (
    known.length > 0 &&
    known.every((target) => {
      if (target.kind === "conversation") {
        const thread = findThread(ctx.queryClient, target.thread_id);
        if (thread) return thread.flagged;
        const selected = ctx.store.getState().selectedMessageId;
        return selected !== null && findSummary(ctx.queryClient, selected)?.flags.flagged === true;
      }
      return (
        target.kind === "message" &&
        findSummary(ctx.queryClient, target.message_id)?.flags.flagged === true
      );
    })
  );
}

/**
 * Whether marking the targets read (`read`) or unread would change
 * anything: some target is unread (or read). Unknown targets count as
 * changeable, so the action stays offered: a whole list asks its folder's
 * unread count when marking read, and is changeable otherwise (rows not
 * loaded are not known).
 */
export function canMark(ctx: WriteContext, targets: readonly MessageTarget[], read: boolean) {
  return targets.some((target) => {
    if (target.kind === "list") return read ? (listFolderUnread(ctx, target) ?? true) : true;
    if (target.kind === "conversation") {
      const thread = findThread(ctx.queryClient, target.thread_id);
      if (!thread) return true;
      return read ? thread.unread_count > 0 : thread.unread_count < thread.message_count;
    }
    const summary = findSummary(ctx.queryClient, target.message_id);
    return summary?.flags.seen !== read;
  });
}
