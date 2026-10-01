/**
 * Feature slot of M3c WP94 for Exchange accounts (contract ADR 0072):
 * shared mailboxes and delegate folders in the folder tree, the room
 * finder in the event editor, the out-of-office (automatic replies)
 * editor, Exchange rule placement badges, the Focused Inbox toggle / view,
 * categories with their colours, To Do lists in the task widget.
 *
 * Only WP94 edits the files in this directory (plus
 * `crates/nail-config/src/widgets/exchange.rs` and
 * `locales/{en,de}/exchange.ftl`). The central registry
 * (`../registry.ts`) merges these exports into the app; see `../README.md`.
 */
import type { Event as NailEvent, ExchangeArea } from "../../api/generated";
import type { Feature, FeatureEventHandler } from "../types";

/** Query keys of this feature. */
export const exchangeKeys = {
  all: ["exchange"] as const,
  account: (accountId: string) => ["exchange", accountId] as const,
  area: (accountId: string, area: ExchangeArea) => ["exchange", accountId, area] as const,
};

/** The core's events that change Exchange state or what it counts. */
export const EXCHANGE_EVENTS = [
  "ExchangeChanged",
  "MailChanged",
  "AccountsChanged",
] as const satisfies readonly NailEvent["type"][];

/** Layout widget kinds → components (`focused-tabs`, `automatic-replies-banner`: WP94). */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};

/** Exchange state changed: refetch the named areas of the account (or everything on mail changes). */
export const handleExchangeEvent: FeatureEventHandler = (event, { queryClient }) => {
  if (event.type === "ExchangeChanged") {
    for (const area of event.areas) {
      void queryClient.invalidateQueries({ queryKey: exchangeKeys.area(event.account_id, area) });
    }
  } else if ((EXCHANGE_EVENTS as readonly string[]).includes(event.type)) {
    void queryClient.invalidateQueries({ queryKey: exchangeKeys.all });
  }
};

/** Handlers for the core's events. */
export const events: Feature["events"] = [handleExchangeEvent];
/** Background components. */
export const effects: Feature["effects"] = [];
