/**
 * Feature slot of P3 WP-actions-ui (contract ADR 0164): Gmail-like actions
 * on conversations. The reader's toolbar, the list row, its context menu and
 * the list / reader keys act on the selected conversation
 * (`ApplyMailAction` with a conversation target: thread + the listed
 * folder; `../../actions/mailTargets.ts`); a card's own actions act on its
 * message. Every action is one batch with one undo: the undo toast
 * (`./Toast.tsx`; `mail.undo`, `z`), the confirmation of "Delete
 * permanently" with the message count (`ResolveMailTargets`,
 * `../../widgets/DeletePermanently.tsx`). The handlers of `mail.archive`,
 * `mail.delete`, `mail.toggle-read`, `mail.mark-read`, `mail.mark-unread`,
 * `mail.toggle-flag`, `mail.move(-to)` and `mail.undo` are built-ins
 * (`../../actions/registry.ts`) running `./run.ts`; this slot registers the
 * toast and the palette descriptions of the new actions.
 * R4 (ADR 0230): `mail.move-out-of-drafts` moves the selected message that
 * sits in a Drafts folder without being a draft (no `\Draft` flag) to the
 * Inbox, never the picked rows or its conversation (real drafts stay); the
 * reader offers it for such messages, which open in the reader rather than
 * the composer. The move window asks before Drafts, Trash and Junk
 * (`./move.ts`, `../../widgets/FolderPicker.tsx`).
 * R5 (ADR 0235): `list.select-all` (Ctrl+A / Cmd+A in the list, the palette,
 * the row's menu) selects the whole list, rows not loaded yet included;
 * `list.clear-selection` (Esc) ends it; `list.toggle-row` (Ctrl+Space) leaves
 * the selected row out or takes it back, the keyboard's Ctrl + click
 * (`../../widgets/messageListControl.ts`). Every action on it sends one list
 * target and asks once with the true count (`./bulk.ts`,
 * `./BulkConfirm.tsx`); texts in `locales/{en,de}/list-selection.ftl`.
 * Texts: `locales/{en,de}/mail-actions.ftl` (labels of what was done:
 * `mail-action-kind-*` in `core-mail-actions.ftl`). The mock core answers
 * the requests (`../../api/mockMailActions.ts`).
 */
import { listSelectionFor, messageTarget } from "../../actions/mailTargets";
import { MAIL_AVAILABILITY as MAIL, type ActionInfo } from "../../actions/palette";
import { currentListControl } from "../../widgets/messageListControl";
import type { Feature } from "../types";
import { BULK_CONFIRM_DIALOG } from "./bulk";
import { BulkConfirmDialog } from "./BulkConfirm";
import { isMisplacedInDrafts, runMailAction } from "./run";
import { MailActionToast } from "./Toast";

/** Selects the whole message list, rows not loaded yet included (R5). */
export const SELECT_ALL_ACTION = "list.select-all";
/** Ends "select all" and drops the picked rows (Esc in the list). */
export const CLEAR_SELECTION_ACTION = "list.clear-selection";

/** Leaves the selected row out of "select all" or takes it back (Ctrl+Space). */
export const TOGGLE_ROW_ACTION = "list.toggle-row";

/** Moves a non-draft out of a Drafts folder into its account's Inbox. */
export const MOVE_OUT_OF_DRAFTS_ACTION = "mail.move-out-of-drafts";

export const widgets: Feature["widgets"] = {};
export const actions: Feature["actions"] = {
  [SELECT_ALL_ACTION]: {
    run: (ctx) => {
      listSelectionFor(ctx.store).getState().selectAll();
    },
    enabled: (ctx) => {
      const { list } = listSelectionFor(ctx.store).getState();
      return list !== null && ctx.store.getState().messageOrder.length > 0;
    },
  },
  [CLEAR_SELECTION_ACTION]: {
    run: (ctx) => {
      listSelectionFor(ctx.store).getState().clear();
    },
    enabled: (ctx) => {
      const { all, picked } = listSelectionFor(ctx.store).getState();
      return all !== null || picked.length > 0;
    },
  },
  [TOGGLE_ROW_ACTION]: {
    run: () => {
      currentListControl()?.toggleRow();
    },
    enabled: () => currentListControl() !== null,
  },
  [MOVE_OUT_OF_DRAFTS_ACTION]: {
    // Only the selected message: the open menu's row, the picked rows or
    // its conversation may hold real drafts, which stay where they are.
    run: async (ctx) => {
      const selected = ctx.store.getState().selectedMessageId;
      if (selected === null || !isMisplacedInDrafts(ctx, selected)) return;
      await runMailAction(
        ctx,
        { targets: [messageTarget(selected)], source: "list" },
        { type: "move", target: { role: "inbox" } },
      );
    },
    enabled: (ctx) => isMisplacedInDrafts(ctx, ctx.store.getState().selectedMessageId),
  },
};

/** Command-palette descriptions of the actions P3 added. */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
  [SELECT_ALL_ACTION]: {
    title: "message-list-action-select-all",
    keywords: "message-list-action-select-all-keywords",
    availability: ["list"],
  },
  [CLEAR_SELECTION_ACTION]: {
    title: "message-list-action-clear-selection",
    keywords: "message-list-action-clear-selection-keywords",
    availability: ["list"],
  },
  [TOGGLE_ROW_ACTION]: {
    title: "message-list-action-toggle-row",
    keywords: "message-list-action-toggle-row-keywords",
    availability: ["list"],
  },
  "mail.mark-read": {
    title: "mail-actions-action-mark-read",
    keywords: "mail-actions-action-mark-read-keywords",
    availability: MAIL,
    icon: "mail-open",
  },
  "mail.mark-unread": {
    title: "mail-actions-action-mark-unread",
    keywords: "mail-actions-action-mark-unread-keywords",
    availability: MAIL,
    icon: "mail",
  },
  [MOVE_OUT_OF_DRAFTS_ACTION]: {
    title: "mail-actions-action-move-out-of-drafts",
    keywords: "mail-actions-action-move-out-of-drafts-keywords",
    availability: MAIL,
    icon: "folder-input",
  },
  "mail.undo": {
    title: "mail-actions-action-undo",
    keywords: "mail-actions-action-undo-keywords",
    icon: "undo-2",
  },
};

/** The confirmation before an action on a whole list. */
export const dialogs: NonNullable<Feature["dialogs"]> = {
  [BULK_CONFIRM_DIALOG]: BulkConfirmDialog,
};
export const windows: NonNullable<Feature["windows"]> = {
  [BULK_CONFIRM_DIALOG]: { confirmation: true },
};

/** The undo toast. */
export const effects: NonNullable<Feature["effects"]> = [MailActionToast];
