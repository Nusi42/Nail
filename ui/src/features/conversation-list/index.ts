/**
 * Feature slot of P2 WP-list (contract ADR 0148): the conversation list
 * (Design A rows, Design B expandable child rows, list keys j / k / → / ←,
 * the selection model over conversations and children, Compact rows). Its
 * widget code lives in `../../widgets/MessageList*.tsx` and
 * `../../widgets/messageListSkin.ts`; this slot holds what the list registers
 * centrally: actions (+ `actionInfo`), settings labels, effects. Texts:
 * `locales/{en,de}/conversation-list.ftl` (shared words in
 * `conversations.ftl`).
 *
 * The contract: `GetConversation` / `useConversation` (`../../state/queries.ts`),
 * `ThreadSummary.participants` with `me` / `unread`, `latest_from`,
 * `ConversationMessage.from_me`; the mock core answers all of it
 * (`../../api/mockMail.ts`; `addMockConversation` in
 * `../../api/mockConversations.ts` adds a conversation across Inbox and
 * Sent). Only WP-list edits the files in this directory. The central registry
 * (`../registry.ts`) merges these exports into the app; see `../README.md`.
 */
import type { Feature } from "../types";

export const widgets: Feature["widgets"] = {};
export const actions: Feature["actions"] = {};
export const dialogs: Feature["dialogs"] = {};
export const events: Feature["events"] = [];
export const effects: Feature["effects"] = [];
