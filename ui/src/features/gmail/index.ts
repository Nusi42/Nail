/**
 * Feature slot of M3b WP84 for Gmail accounts (contract ADR 0065): label
 * management (create, rename, recolour, delete), label-based views (a
 * label's messages across folders), Gmail categories as a filterable
 * property, the Gmail account setup (OAuth with the Gmail API).
 *
 * Only WP84 edits the files in this directory (plus
 * `crates/nail-config/src/widgets/gmail.rs` and `locales/{en,de}/gmail.ftl`).
 * The central registry (`../registry.ts`) merges these exports into the
 * app; see `../README.md`.
 */
import type { Event as NailEvent } from "../../api/generated";
import type { Feature, FeatureEventHandler } from "../types";

/** Query keys of this feature. */
export const gmailKeys = {
  all: ["gmail"] as const,
  labels: (accountId: string) => ["gmail", "labels", accountId] as const,
};

/** The core's events that change Gmail labels or their counts. */
export const GMAIL_EVENTS = [
  "GmailLabelsChanged",
  "MailChanged",
  "AccountsChanged",
] as const satisfies readonly NailEvent["type"][];

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};

/** Labels (or messages, whose counts they show) changed: refetch. */
export const handleGmailEvent: FeatureEventHandler = (event, { queryClient }) => {
  if (event.type === "GmailLabelsChanged") {
    void queryClient.invalidateQueries({ queryKey: gmailKeys.labels(event.account_id) });
  } else if ((GMAIL_EVENTS as readonly string[]).includes(event.type)) {
    void queryClient.invalidateQueries({ queryKey: gmailKeys.all });
  }
};

/** Handlers for the core's events. */
export const events: Feature["events"] = [handleGmailEvent];
/** Background components. */
export const effects: Feature["effects"] = [];
