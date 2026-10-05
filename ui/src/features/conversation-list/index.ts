/**
 * Feature slot of P2 WP-list (contract ADR 0148): the conversation list
 * (Design A rows, Design B expandable child rows, list keys j / k / → / ←,
 * the selection model over conversations and children, Compact rows). Its
 * widget code lives in `../../widgets/MessageList*.tsx`,
 * `../../widgets/messageListModel.ts` and `../../widgets/messageListSkin.ts`;
 * this slot holds what the list registers centrally: the actions
 * `list.expand` / `list.collapse` (→ / ← in the `list` key context of
 * `defaults/layout.kdl`), which act on the expandable list that holds the
 * focus (`../../widgets/messageListControl.ts`). j / k stay `mail.next` /
 * `mail.previous`; an expandable list steps through its own rows with them
 * (`listStep` in `MessageList.tsx`). Texts:
 * `locales/{en,de}/conversation-list.ftl` (shared words in
 * `conversations.ftl`).
 */
import type { ActionInfo } from "../../actions/palette";
import { currentListControl } from "../../widgets/messageListControl";
import type { Feature } from "../types";

/** Opens the selected conversation, or moves into it when open (→). */
export const LIST_EXPAND_ACTION = "list.expand";
/** Closes the selected conversation, or moves to it from a child row (←). */
export const LIST_COLLAPSE_ACTION = "list.collapse";

export const widgets: Feature["widgets"] = {};
export const actions: Feature["actions"] = {
  [LIST_EXPAND_ACTION]: {
    run: () => {
      currentListControl()?.expand();
    },
    enabled: () => currentListControl() !== null,
  },
  [LIST_COLLAPSE_ACTION]: {
    run: () => {
      currentListControl()?.collapse();
    },
    enabled: () => currentListControl() !== null,
  },
};
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
  [LIST_EXPAND_ACTION]: {
    title: "conversation-list-action-expand",
    keywords: "conversation-list-action-expand-keywords",
    availability: ["list"],
  },
  [LIST_COLLAPSE_ACTION]: {
    title: "conversation-list-action-collapse",
    keywords: "conversation-list-action-collapse-keywords",
    availability: ["list"],
  },
};
export const dialogs: Feature["dialogs"] = {};
export const events: Feature["events"] = [];
export const effects: Feature["effects"] = [];
