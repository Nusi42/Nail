/**
 * The move window (`mail.move`, `v`) moves what was the target when it
 * opened: the selected conversation (its messages in the listed folder),
 * the picked rows, or a card's message (P3, ADR 0164).
 *
 * R4 (ADR 0230): the window leaves out the folder the targets are already
 * in, and moving into Drafts, Trash or Junk asks first with the number of
 * messages the core resolved the targets to (`ResolveMailTargets`);
 * confirming moves exactly those messages. A whole list ("select all",
 * ADR 0235) asks once with the true count (`./bulk.ts`).
 */
import { currentTargets, listFolderOf, type ActionTargets } from "../../actions/mailTargets";
import type {
  Folder,
  FolderId,
  FolderRole,
  MailAction,
  MailTargetResolution,
  MessageTarget,
} from "../../api/generated";
import { findSummary, type WriteContext } from "../../state/mailOps";
import type { UiStore } from "../../state/uiStore";
import { UiStoreMap } from "../../state/uiStore";
import { runMailAction, runOptionsOf, type RunOptions } from "./run";

const pending = new UiStoreMap<ActionTargets>();

/** Remembers the targets of the move window that opens now. */
export function setPendingMove(store: UiStore, targets: ActionTargets): void {
  pending.set(store, targets);
}

/** The targets the move window acts on: those it opened with, else the current ones. */
export function pendingMove(store: UiStore): ActionTargets {
  return pending.get(store) ?? currentTargets(store);
}

/**
 * Per special-use role, whether a move into its folder asks first: mail
 * moved there looks gone (Trash, Junk) or is mistaken for an unfinished
 * draft (Drafts). Every role decides, so a new one cannot be forgotten.
 */
const ASKS_BEFORE_MOVE = {
  inbox: false,
  sent: false,
  drafts: true,
  archive: false,
  junk: true,
  trash: true,
  other: false,
} as const satisfies Record<FolderRole, boolean>;

/** A role whose folder asks before a move into it. */
export type ConfirmedMoveRole = {
  [R in FolderRole]: (typeof ASKS_BEFORE_MOVE)[R] extends true ? R : never;
}[FolderRole];

function asksBeforeMove(role: FolderRole): role is ConfirmedMoveRole {
  return ASKS_BEFORE_MOVE[role];
}

/** The role a move into `folder` asks about, `null` when it moves at once. */
export function confirmedMoveRole(folder: Pick<Folder, "role">): ConfirmedMoveRole | null {
  return asksBeforeMove(folder.role) ? folder.role : null;
}

/**
 * The folder every target is in (a conversation's listed folder, a
 * message's own folder); `null` when they are in several, while searching
 * or when a message is not cached.
 */
export function sourceFolderOf(
  ctx: Pick<WriteContext, "queryClient">,
  targets: readonly MessageTarget[],
): FolderId | null {
  const folders = new Set(
    targets.map((target) =>
      target.kind === "conversation"
        ? target.folder_id
        : target.kind === "list"
          ? listFolderOf(target.list)
          : (findSummary(ctx.queryClient, target.message_id)?.folder_id ?? null),
    ),
  );
  const [only] = folders;
  return folders.size === 1 && only !== undefined ? only : null;
}

const moveAction = (folderId: FolderId): MailAction => ({
  type: "move",
  target: { folder_id: folderId },
});

/** The messages a move of the window's targets to `folderId` would touch (the question's count). */
export async function resolveMove(
  ctx: WriteContext,
  folderId: FolderId,
): Promise<MailTargetResolution> {
  const { targets } = pendingMove(ctx.store);
  const { resolution } = await ctx.client.request(
    { type: "ResolveMailTargets", targets: [...targets], action: moveAction(folderId) },
    "MailTargets",
  );
  return resolution;
}

/**
 * Moves the move window's targets to `folderId`; `false` when the core
 * refused. With a confirmed `resolution` exactly its messages move, so
 * nothing that arrived while the question was open moves unasked.
 */
export async function moveToFolder(
  ctx: WriteContext,
  folderId: FolderId,
  resolution?: MailTargetResolution,
): Promise<boolean> {
  const targets = pendingMove(ctx.store);
  pending.delete(ctx.store);
  const options: RunOptions =
    resolution === undefined || resolution.messages === 0 ? {} : runOptionsOf(resolution);
  const result = await runMailAction(ctx, targets, moveAction(folderId), options);
  return result !== null;
}
