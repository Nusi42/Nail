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
 * Texts: `locales/{en,de}/mail-actions.ftl` (labels of what was done:
 * `mail-action-kind-*` in `core-mail-actions.ftl`). The mock core answers
 * the requests (`../../api/mockMailActions.ts`).
 */
import { MAIL_AVAILABILITY as MAIL, type ActionInfo } from "../../actions/palette";
import type { Feature } from "../types";
import { MailActionToast } from "./Toast";

export const widgets: Feature["widgets"] = {};
export const actions: Feature["actions"] = {};

/** Command-palette descriptions of the actions P3 added. */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
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
  "mail.undo": {
    title: "mail-actions-action-undo",
    keywords: "mail-actions-action-undo-keywords",
    icon: "undo-2",
  },
};

/** The undo toast. */
export const effects: NonNullable<Feature["effects"]> = [MailActionToast];
