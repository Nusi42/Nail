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
import type { ActionInfo } from "../../actions/palette";
import type { Feature } from "../types";
import { conversationActionInfo, conversationActions } from "./actions";
import { ReaderOffer } from "./ReaderOffer";

/** The card feed is the `thread-view` widget (`mode="cards"`), registered with the core widgets. */
export const widgets: Feature["widgets"] = {};
/** The feed's keys (`conversation.*`). */
export const actions: Feature["actions"] = conversationActions;
/** Command-palette descriptions of the feed's keys. */
export const actionInfo: Readonly<Record<string, ActionInfo>> = conversationActionInfo;
export const dialogs: Feature["dialogs"] = {};
export const events: Feature["events"] = [];
/** The offer to turn a strip-era reader stack into the card feed. */
export const effects: Feature["effects"] = [ReaderOffer];
