/**
 * Feature slot of M4c WP124 (contract ADR 0094): import (mbox, EML folders,
 * Maildir, Thunderbird profiles with account / filter proposals, ICS,
 * vCard), export, backup / verify / restore (dry run first) as wizards with
 * progress and reports, and the diagnostics bundle (user-initiated, shows
 * exactly what is included); future widgets `import-export`,
 * `backup-restore`, `diagnostics` (`nail-config/src/widgets/portability.rs`).
 *
 * Only WP124 edits the files in this directory (plus
 * `locales/{en,de}/portability.ftl`). The central registry
 * (`../registry.ts`) merges these exports into the app; see `../README.md`.
 * The contract's texts are in `locales/en/core-portability.ftl`.
 */
import type { Event as NailEvent } from "../../api/generated";
import type { Feature, FeatureEventHandler } from "../types";

/** Query keys of this feature. */
export const portabilityKeys = {
  all: ["portability"] as const,
  jobs: ["portability", "jobs"] as const,
  sources: ["portability", "sources"] as const,
};

/** The core's events that change what this feature shows. */
export const PORTABILITY_EVENTS = [
  "PortabilityJobProgress",
  "PortabilityJobFinished",
] as const satisfies readonly NailEvent["type"][];

/** Layout widget kinds → components (WP124). */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};

/** Keeps the job list fresh. */
export const handlePortabilityEvent: FeatureEventHandler = (event, { queryClient }) => {
  if ((PORTABILITY_EVENTS as readonly string[]).includes(event.type)) {
    void queryClient.invalidateQueries({ queryKey: portabilityKeys.jobs });
  }
};

/** Handlers for the core's events. */
export const events: Feature["events"] = [handlePortabilityEvent];
/** Background components. */
export const effects: Feature["effects"] = [];
