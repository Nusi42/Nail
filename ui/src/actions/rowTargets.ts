/**
 * Which target a message-list row stands for (P3, ADR 0164; R5, ADR 0235):
 * one rule for the rows picked one by one, the rows of "select all" and the
 * mock core's whole lists, pinned against the core's
 * `MailList::row_target` by the `mail-actions` ts-vectors. No imports
 * besides the contract's types, so the mock core can use it.
 */
import type { FolderId, MailList, MessageId, MessageTarget, ThreadId } from "../api/generated";

/** How the list's rows map to targets. */
export interface ListScope {
  /** The rows are conversations (`threads` is not `off`). */
  conversations: boolean;
  /** The folder the rows are listed in ({@link rowFolderOf}); `null` while searching. */
  folderId: FolderId | null;
}

/** A target that is one row (a list target aside). */
export type RowTarget = Exclude<MessageTarget, { kind: "list" }>;

/** The target that names one message. */
export const messageTarget = (id: MessageId): RowTarget => ({
  kind: "message",
  message_id: id,
});

/** The target a row stands for. */
export function rowTarget(
  row: { messageId: MessageId; threadId: ThreadId | null; child: boolean },
  scope: ListScope,
): RowTarget {
  if (!row.child && row.threadId !== null && scope.conversations) {
    return { kind: "conversation", thread_id: row.threadId, folder_id: scope.folderId };
  }
  return messageTarget(row.messageId);
}

/**
 * The folder a list's conversation rows are listed in (`ListScope.folderId`;
 * `MailList::row_folder_id`, pinned by the `mail-actions` ts-vectors).
 */
export function rowFolderOf(list: MailList | null): FolderId | null {
  if (list === null) return null;
  return list.source === "search" ? list.listed_folder_id : list.folder_id;
}

/** Whether a list's rows stand for conversations (`MailList::conversation_rows`). */
function conversationRows(list: MailList): boolean {
  return list.source === "search" ? list.conversations : list.source === "conversations";
}

/**
 * The target a row of a whole list stands for (`MailList::row_target`,
 * pinned by the `mail-actions` ts-vectors): {@link rowTarget} with the
 * list's own scope, so the core resolves "select all" exactly as the rows
 * picked one by one.
 */
export function listRowTarget(
  list: MailList,
  row: { messageId: MessageId; threadId: ThreadId | null },
): RowTarget {
  return rowTarget(
    { ...row, child: false },
    { conversations: conversationRows(list), folderId: rowFolderOf(list) },
  );
}
