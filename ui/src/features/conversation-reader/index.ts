/**
 * Feature slot of P2 WP-reader (contract ADR 0148): the reader's card feed
 * (Design A: cards oldest → newest reusing `message-view`, folds,
 * `role="feed"`, keys j / k, Enter / o, ; / Shift+;, n / p, r / a / f per
 * card) and the reader stack's `layout.kdl` migration for user layouts. Its
 * widget code lives in `../../widgets/ThreadView*.tsx` and
 * `../../widgets/threadViewSkin.ts`; this slot holds what the reader
 * registers centrally: actions (+ `actionInfo`), effects (e.g. the migration
 * offer). Texts: `locales/{en,de}/conversation-reader.ftl` (shared words in
 * `conversations.ftl`).
 *
 * The contract: `GetConversation` / `useConversation` (`../../state/queries.ts`),
 * `ThreadSummary.participants` with `me` / `unread`, `latest_from`,
 * `ConversationMessage.from_me`; the mock core answers all of it
 * (`../../api/mockMail.ts`; `addMockConversation` in
 * `../../api/mockConversations.ts` adds a conversation across Inbox and
 * Sent). Only WP-reader edits the files in this directory. The central registry
 * (`../registry.ts`) merges these exports into the app; see `../README.md`.
 */
import type { Feature } from "../types";

export const widgets: Feature["widgets"] = {};
export const actions: Feature["actions"] = {};
export const dialogs: Feature["dialogs"] = {};
export const events: Feature["events"] = [];
export const effects: Feature["effects"] = [];
