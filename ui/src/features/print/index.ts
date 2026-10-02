/**
 * Feature slot of M4c WP124 (contract ADR 0094): printing and saving as PDF
 * — print jobs (`CreatePrintJob` for a message, thread, event or agenda),
 * the print view (route `#/print/<job id>`, rendered with the theme's
 * `print.*` parts, reports `PrintViewReady`), the system print dialog and,
 * on the desktop, `SavePrintPdf` (WebView2 `PrintToPdf` in the shell).
 *
 * Only WP124 edits the files in this directory (plus
 * `locales/{en,de}/print.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`.
 */
import type { Feature } from "../types";

export { printViewRoute } from "./mock";

/** Query keys of this feature. */
export const printKeys = {
  all: ["print"] as const,
  job: (jobId: string) => ["print", "job", jobId] as const,
};

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers (WP124: `print.message`, `print.save-pdf`, …). */
export const actions: Feature["actions"] = {};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};

/** Handlers for the core's events. */
export const events: Feature["events"] = [];
/** Background components. */
export const effects: Feature["effects"] = [];
