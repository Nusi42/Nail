/**
 * Feature slot of M2c WP54 for server-side rules and vacation replies
 * (contract ADR 0043): where each rule runs (server / Nail / Nail only,
 * with the reasons), the ManageSieve state per account and the vacation
 * reply editor.
 *
 * Only WP54 edits the files in this directory (plus
 * `crates/nail-config/src/widgets/sieve.rs` for widget kinds and
 * `locales/{en,de}/sieve.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`.
 */
import type { Feature, FeatureEventHandler } from "../types";
import { SieveStatusWidget, VacationEditorWidget, sieveKeys } from "./Sieve";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "sieve-status": SieveStatusWidget,
  "vacation-editor": VacationEditorWidget,
};
/** Action ids → handlers. */
export const actions: Feature["actions"] = {};
/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};

/** Keeps the Sieve state current (uploads, rules and accounts change it). */
export const handleSieveEvent: FeatureEventHandler = (event, { queryClient }) => {
  switch (event.type) {
    case "SieveStatusChanged":
    case "RulesChanged":
    case "AccountsChanged":
      void queryClient.invalidateQueries({ queryKey: sieveKeys.status });
      break;
  }
};

/** Handlers for the core's events. */
export const events: Feature["events"] = [handleSieveEvent];
/** Background components. */
export const effects: Feature["effects"] = [];
