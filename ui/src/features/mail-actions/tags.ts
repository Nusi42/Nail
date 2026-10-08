/**
 * Label / category windows (`gmail.apply-labels`, `exchange.apply-categories`)
 * act like Gmail's label menu: on what was the target when they opened —
 * the selected conversation (every message of it), the picked rows, or a
 * card's message (P3, ADR 0169).
 */
import { useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { currentTargets, type ActionTargets } from "../../actions/mailTargets";
import type { NailClient } from "../../api/client";
import type { MailActionResult, MessageSummary } from "../../api/generated";
import { findSummary, type WriteContext } from "../../state/mailOps";
import { useBusyAction } from "../../state/busyAction";
import { useClient } from "../../state/queries";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";
import { runMailAction, type TagNoun } from "./run";

const pending = new WeakMap<UiStore, ActionTargets>();

/** Remembers the targets of the label window that opens now (the current ones). */
export function rememberTagTargets(store: UiStore): void {
  pending.set(store, currentTargets(store));
}

/** The targets the label window acts on: those it opened with, else the current ones. */
export function pendingTagTargets(store: UiStore): ActionTargets {
  return pending.get(store) ?? currentTargets(store);
}

/**
 * Adds and removes tags on the label window's targets as one undoable
 * batch; `null` when the core refused (the toast says why).
 */
export async function applyTags(
  ctx: WriteContext,
  add: string[],
  remove: string[],
  noun: TagNoun = "tag",
): Promise<MailActionResult | null> {
  const targets = pendingTagTargets(ctx.store);
  pending.delete(ctx.store);
  return runMailAction(ctx, targets, { type: "tags", add, remove }, { tagNoun: noun });
}

/** How many of the targets' messages carry each tag (lower-cased). */
export interface TagCounts {
  /** Messages looked at. */
  total: number;
  counts: ReadonlyMap<string, number>;
  /** The spellings each tag has on the messages (removing a tag removes each). */
  spellings: ReadonlyMap<string, readonly string[]>;
}

/** No messages (yet): every tag unchecked. */
export const NO_TAGS: TagCounts = { total: 0, counts: new Map(), spellings: new Map() };

/** A tag on all, some or none of the targets' messages (a check box's state). */
export type TagMark = "all" | "some" | "none";

/**
 * The messages behind the targets as a tag change reaches them (every
 * message of a conversation: `GetConversation`; a message: its cached
 * summary).
 */
export async function targetMessages(
  client: NailClient,
  queryClient: QueryClient,
  { targets }: ActionTargets,
): Promise<MessageSummary[]> {
  const out = new Map<string, MessageSummary>();
  for (const target of targets) {
    if (target.kind === "message") {
      const found = findSummary(queryClient, target.message_id);
      if (found) out.set(found.id, found);
      continue;
    }
    const answer = await client.request(
      { type: "GetConversation", thread_id: target.thread_id, folder_id: target.folder_id },
      "Conversation",
    );
    for (const m of answer.messages) out.set(m.id, m);
  }
  return [...out.values()];
}

/** Counts the tags of `messages`. */
export function countTags(messages: readonly MessageSummary[]): TagCounts {
  const counts = new Map<string, number>();
  const spellings = new Map<string, string[]>();
  for (const m of messages) {
    for (const tag of new Set(m.tags.map((t) => t.toLowerCase()))) {
      counts.set(tag, (counts.get(tag) ?? 0) + 1);
    }
    for (const tag of m.tags) {
      const known = spellings.get(tag.toLowerCase()) ?? [];
      if (!known.includes(tag)) spellings.set(tag.toLowerCase(), [...known, tag]);
    }
  }
  return { total: messages.length, counts, spellings };
}

/** Whether all, some or none of the messages carry `tag`. */
export function tagMark(counts: TagCounts, tag: string): TagMark {
  const n = counts.counts.get(tag.toLowerCase()) ?? 0;
  if (n === 0) return "none";
  return n >= counts.total ? "all" : "some";
}

/**
 * The change the window's check boxes ask for: a tag the user checked is
 * added where it is missing, one the user cleared is removed where it is;
 * an untouched mixed tag stays as it is on each message.
 */
export function tagChange(
  tags: readonly string[],
  counts: TagCounts,
  decided: ReadonlyMap<string, boolean>,
): { add: string[]; remove: string[] } {
  const add: string[] = [];
  const remove: string[] = [];
  for (const tag of tags) {
    const wanted = decided.get(tag);
    if (wanted === undefined) continue;
    const mark = tagMark(counts, tag);
    if (wanted && mark !== "all") add.push(tag);
    if (!wanted && mark !== "none") {
      for (const spelling of counts.spellings.get(tag.toLowerCase()) ?? [tag]) {
        if (!remove.includes(spelling)) remove.push(spelling);
      }
    }
  }
  return { add, remove };
}

/**
 * The label / category window's state: the targets it opened with (kept,
 * so a selection change behind the non-modal window changes nothing) and
 * the tag counts over their messages.
 */
export function useTargetTags(): { targets: ActionTargets; counts: TagCounts | undefined } {
  const store = useUiStoreApi();
  const client = useClient();
  const queryClient = useQueryClient();
  const [targets] = useState(() => pendingTagTargets(store));
  const query = useQuery({
    queryKey: ["tag-targets", targets.targets],
    queryFn: async () => countTags(await targetMessages(client, queryClient, targets)),
    gcTime: 0,
  });
  return { targets, counts: query.data };
}

/**
 * A label / category window's state: the message it opened for (a selection
 * change behind it changes nothing), the user's decision per tag, and
 * `apply`, which writes the decisions as one undoable batch on the whole
 * conversation (or the picked rows / the card) and closes the window.
 */
export function useTagWindow(noun: TagNoun, close: () => void) {
  const client = useClient();
  const queryClient = useQueryClient();
  const store = useUiStoreApi();
  const [messageId] = useState(() => store.getState().selectedMessageId);
  const summary = useMemo(
    () => (messageId ? findSummary(queryClient, messageId) : undefined),
    [messageId, queryClient],
  );
  const { counts } = useTargetTags();
  const [decided, setDecided] = useState<ReadonlyMap<string, boolean>>(new Map());
  const { busy, problem, run } = useBusyAction();
  return {
    store,
    summary,
    busy,
    problem,
    /** Whether all, some or none of the targets carry `tag`. */
    mark: (tag: string) => tagMark(counts ?? NO_TAGS, tag),
    /** What the user chose for `tag`, if anything. */
    decision: (tag: string) => decided.get(tag),
    decide: (tag: string, checked: boolean) => {
      setDecided(new Map(decided).set(tag, checked));
    },
    /** Applies the decisions over `tags` (closes at once when nothing changes). */
    apply: async (tags: readonly string[]) => {
      const { add, remove } = tagChange(tags, counts ?? NO_TAGS, decided);
      if (add.length === 0 && remove.length === 0) {
        close();
        return;
      }
      await run(async () => {
        if ((await applyTags({ client, store, queryClient }, add, remove, noun)) !== null) close();
      });
    },
  };
}
