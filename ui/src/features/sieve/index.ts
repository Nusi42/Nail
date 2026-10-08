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
export const events: NonNullable<Feature["events"]> = [handleSieveEvent];

/**
 * Settings labels per widget kind, for the settings search (M5b WP163, ADR
 * 0109): Fluent ids of the setting labels each widget shows.
 */
export const settings: NonNullable<Feature["settings"]> = {
  "sieve-status": {
    labels: [
      "sieve-settings-tab",
      "sieve-status-label",
      "sieve-connection-title",
      "sieve-enabled",
      "sieve-host",
      "sieve-port",
      "sieve-script-name",
      "sieve-rules-title",
      "sieve-script-label",
      "sieve-other-scripts",
      "sieve-vacation-state",
    ],
    keywords: "settings-keywords-sieve-status",
  },
  "vacation-editor": {
    labels: [
      "vacation-settings-tab",
      "vacation-label",
      "vacation-enabled",
      "vacation-start",
      "vacation-end",
      "vacation-subject",
      "vacation-body",
      "vacation-from",
      "vacation-addresses",
      "vacation-days",
      "vacation-skip-lists",
    ],
    keywords: "settings-keywords-vacation-editor",
  },
};
