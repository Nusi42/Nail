/**
 * Feature slot of P3 WP-actions-ui (contract ADR 0164): Gmail-like actions
 * on conversations. The reader's toolbar, the list row, its context menu and
 * the list / reader keys act on the selected conversation
 * (`ApplyMailAction` with a conversation target: thread + the listed
 * folder); a card's own actions act on its message. Every action is one
 * batch with one undo: the undo toast (`mail.undo`, `z`), the confirmation
 * of "Delete permanently" with the message count (`ResolveMailTargets`).
 * Texts: `locales/{en,de}/mail-actions.ftl` (labels of what was done:
 * `mail-action-kind-*` in `core-mail-actions.ftl`). The mock core answers
 * the requests (`../../api/mockMailActions.ts`).
 */
import type { Feature } from "../types";

export const widgets: Feature["widgets"] = {};
export const actions: Feature["actions"] = {};
export const dialogs: Feature["dialogs"] = {};
export const events: Feature["events"] = [];
export const effects: Feature["effects"] = [];
