/**
 * Structure of this feature's parts every theme needs (like
 * `SYSTEM_BASE_CSS`): lists without bullets, rows of buttons that wrap,
 * long paths and hashes that wrap, the diagnostics text previews keeping
 * their line breaks. Colours, spacing and fonts come from `theme.kdl`
 * (states via `data-state` / `data-severity` / `data-tone`).
 *
 * The integrator appends this to the compiled theme's base CSS
 * (`ui/src/theme/compile.ts`, next to `SYSTEM_BASE_CSS`).
 */

import { FIELD_BASIS } from "../../theme/declarations";

/** Lists that show no bullets and no indentation. */
export const PORTABILITY_PLAIN_LISTS = [
  "portability.choices",
  "portability.counts",
  "portability.preview-counts",
  "portability.issues.list",
  "portability.found",
  "portability.proposal-list",
  "portability.manifest.items",
  "portability.jobs.list",
  "diagnostics.entries",
  "diagnostics.entry.redactions",
  "diagnostics.issues",
];

/** Rows of buttons / chips that wrap. */
export const PORTABILITY_ROWS = [
  "wizard.buttons",
  "portability.buttons",
  "path-field",
  "portability.found.item",
  "portability.job",
  "portability.issue",
  "portability.counts",
  "portability.preview-counts",
  "diagnostics.entry",
  "diagnostics.entry.redactions",
];

const sel = (parts: readonly string[]) => parts.map((p) => `[data-part="${p}"]`).join(", ");

export const PORTABILITY_BASE_CSS = `${sel(PORTABILITY_PLAIN_LISTS)} { list-style: none; margin: 0; padding: 0; }
${sel(PORTABILITY_ROWS)} { display: flex; flex-wrap: wrap; align-items: center; }
[data-part="path-field"] > [data-part="form.input"] { flex: 1 1 ${FIELD_BASIS}; min-width: 0; }
[data-part="portability.issue.text"], [data-part="diagnostics.entry.name"], [data-part="portability.job.path"] { flex: 1 1 auto; min-width: 0; }
[data-part="portability.issue.details"], [data-part="diagnostics.entry.details"] { flex: 1 1 100%; }
[data-part="portability.issue.detail"], [data-part="portability.job.path"], [data-part="portability.report.path"], [data-part="diagnostics.saved.hash"], [data-part="diagnostics.saved.text"] { overflow-wrap: anywhere; }
[data-part="diagnostics.entry.text"] { white-space: pre-wrap; overflow-wrap: anywhere; margin: 0; max-height: ${FIELD_BASIS}; overflow: auto; }
[data-part="portability.progress.bar"], [data-part="portability.job.progress"] { width: 100%; }
[data-part="portability.plan"] { border-collapse: collapse; width: 100%; }
[data-part="portability.plan.caption"] { text-align: start; }
`;
