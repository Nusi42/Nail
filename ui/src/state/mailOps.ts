/**
 * Writes on messages: flags, move, delete. Each one is a request the core
 * turns into a queued operation (the operation queue is the core's); the UI
 * only mirrors the expected result in its caches at once, so the list reacts
 * immediately, and refetches when the core reports the real state
 * (`MailChanged`, `OpStatus`).
 */
import type { InfiniteData, QueryClient } from "@tanstack/react-query";
import { NailError, type NailClient } from "../api/client";
import type {
  FlagChange,
  MessageId,
  MessageSummary,
  MoveTarget,
  Response,
  SearchHit,
} from "../api/generated";
import { queryKeys } from "./queries";
import type { UiStore } from "./uiStore";

export interface WriteContext {
  client: NailClient;
  store: UiStore;
  queryClient: QueryClient;
}

type MessagesPage = Extract<Response, { type: "Messages" }>;
type ThreadResponse = Extract<Response, { type: "Thread" }>;
interface SearchPage {
  hits: SearchHit[];
}

/** Looks a message summary up in any list, search or thread cache. */
export function findSummary(queryClient: QueryClient, id: MessageId): MessageSummary | undefined {
  for (const [, data] of queryClient.getQueriesData<InfiniteData<MessagesPage>>({
    queryKey: queryKeys.allMessages,
  })) {
    for (const page of data?.pages ?? []) {
      const found = page.messages.find((m) => m.id === id);
      if (found) return found;
    }
  }
  for (const [, data] of queryClient.getQueriesData<InfiniteData<SearchPage>>({
    queryKey: queryKeys.allSearches,
  })) {
    for (const page of data?.pages ?? []) {
      const found = page.hits.find((h) => h.message.id === id);
      if (found) return found.message;
    }
  }
  for (const [, data] of queryClient.getQueriesData<ThreadResponse>({
    queryKey: queryKeys.allThreadDetails,
  })) {
    const found = data?.messages.find((m) => m.id === id);
    if (found) return found;
  }
  return undefined;
}

/**
 * Rewrites cached summaries of `ids` with `patch`; a `null` result removes
 * the message from lists (moved or deleted).
 */
export function patchSummaries(
  queryClient: QueryClient,
  ids: readonly MessageId[],
  patch: (m: MessageSummary) => MessageSummary | null,
): void {
  const wanted = new Set(ids);
  const apply = (list: MessageSummary[]) =>
    list.flatMap((m) => {
      if (!wanted.has(m.id)) return [m];
      const next = patch(m);
      return next ? [next] : [];
    });
  queryClient.setQueriesData<InfiniteData<MessagesPage>>(
    { queryKey: queryKeys.allMessages },
    (data) =>
      data && {
        ...data,
        pages: data.pages.map((page) => {
          const messages = apply(page.messages);
          return {
            ...page,
            messages,
            total: page.total - (page.messages.length - messages.length),
          };
        }),
      },
  );
  queryClient.setQueriesData<InfiniteData<SearchPage>>(
    { queryKey: queryKeys.allSearches },
    (data) =>
      data && {
        ...data,
        pages: data.pages.map((page) => ({
          ...page,
          hits: page.hits.flatMap((hit) => {
            if (!wanted.has(hit.message.id)) return [hit];
            const next = patch(hit.message);
            return next ? [{ ...hit, message: next }] : [];
          }),
        })),
      },
  );
  queryClient.setQueriesData<ThreadResponse>(
    { queryKey: queryKeys.allThreadDetails },
    (data) => data && { ...data, messages: apply(data.messages) },
  );
}

/** Refetches everything a write may have changed. */
function refresh(queryClient: QueryClient): void {
  void queryClient.invalidateQueries({ queryKey: queryKeys.allFolders });
  void queryClient.invalidateQueries({ queryKey: queryKeys.allThreads });
}

async function queued(
  ctx: WriteContext,
  request: Parameters<NailClient["request"]>[0],
  quiet = false,
) {
  try {
    const { op_ids } = await ctx.client.request(request, "Queued");
    ctx.store.getState().opQueued(op_ids);
    refresh(ctx.queryClient);
  } catch (error) {
    // Undo the optimistic change: show the core's state again.
    if (!quiet) {
      ctx.store.setState({ opError: error instanceof NailError ? error.code : "internal" });
    }
    void ctx.queryClient.invalidateQueries();
    throw error;
  }
}

/** Sets flags (absolute values) on messages. */
export async function setFlags(
  ctx: WriteContext,
  ids: readonly MessageId[],
  change: Partial<Pick<FlagChange, "seen" | "flagged" | "answered">>,
  options: { quiet?: boolean } = {},
): Promise<void> {
  if (ids.length === 0) return;
  const full: FlagChange = {
    seen: change.seen ?? null,
    flagged: change.flagged ?? null,
    answered: change.answered ?? null,
    draft: null,
    add_tags: [],
    remove_tags: [],
  };
  patchSummaries(ctx.queryClient, ids, (m) => ({
    ...m,
    flags: {
      ...m.flags,
      seen: full.seen ?? m.flags.seen,
      flagged: full.flagged ?? m.flags.flagged,
      answered: full.answered ?? m.flags.answered,
    },
  }));
  await queued(
    ctx,
    { type: "UpdateFlags", message_ids: [...ids], change: full },
    options.quiet ?? false,
  );
}

/** The id to select after `ids` leave the list: the next one, else the previous one. */
export function nextSelection(
  order: readonly MessageId[],
  selected: MessageId | null,
  removed: readonly MessageId[],
): MessageId | null {
  if (selected === null || !removed.includes(selected)) return selected;
  const index = order.indexOf(selected);
  const gone = new Set(removed);
  for (let i = index + 1; i < order.length; i++) {
    const id = order[i] as MessageId;
    if (!gone.has(id)) return id;
  }
  for (let i = index - 1; i >= 0; i--) {
    const id = order[i] as MessageId;
    if (!gone.has(id)) return id;
  }
  return null;
}

function removeFromLists(ctx: WriteContext, ids: readonly MessageId[]): void {
  const ui = ctx.store.getState();
  const next = nextSelection(ui.messageOrder, ui.selectedMessageId, ids);
  patchSummaries(ctx.queryClient, ids, () => null);
  ui.setMessageOrder(ui.messageOrder.filter((id) => !ids.includes(id)));
  ui.selectMessage(next);
}

/** Moves messages to a folder or to the folder with a role (e.g. archive). */
export async function moveMessages(
  ctx: WriteContext,
  ids: readonly MessageId[],
  target: MoveTarget,
): Promise<void> {
  if (ids.length === 0) return;
  removeFromLists(ctx, ids);
  await queued(ctx, { type: "MoveMessages", message_ids: [...ids], target });
}

/** Deletes messages: to Trash, or for good. */
export async function deleteMessages(
  ctx: WriteContext,
  ids: readonly MessageId[],
  permanent: boolean,
): Promise<void> {
  if (ids.length === 0) return;
  removeFromLists(ctx, ids);
  await queued(ctx, { type: "DeleteMessages", message_ids: [...ids], permanent });
}
