/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/portability.ftl` (texts the
 * contract names — formats, job kinds / phases / states, issues, backup
 * items, diagnostics items — are in `locales/en/core-portability.ftl`);
 * ids built at runtime are listed by `runtimeIds`.
 */
import type { ExportSource } from "../../api/generated";
import {
  BACKUP_ITEMS,
  COUNT_KEYS,
  countId,
  DIAGNOSTICS_ITEMS,
  EXPORT_FORMATS,
  IMPORT_FORMATS,
  ISSUE_CODES,
  JOB_KINDS,
  JOB_PHASES,
  JOB_STATES,
  MBOX_VARIANTS,
  REDACTIONS,
  RESTORE_EFFECTS,
  SEVERITIES,
  backupItemId,
  diagnosticsItemId,
  exportFormatId,
  importFormatId,
  issueCodeId,
  jobKindId,
  jobPhaseId,
  jobStateId,
  mboxVariantId,
  redactionId,
  restoreEffectId,
  severityId,
} from "./model";
import { PORTABILITY_DIALOGS, REPORT_DIALOG } from "./names";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["portability-"];

/** Literals with those prefixes that are not message ids (part, widget, dialog names). */
export const notMessages: readonly string[] = [
  REPORT_DIALOG,
  // Contract values that match other features' prefixes (`pim-`, `system-`).
  "pim-local",
  "system-info",
];

const EXPORT_SOURCES: readonly ExportSource["type"][] = [
  "folders",
  "messages",
  "calendars",
  "address-books",
];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    "portability-title",
    "m4c-not-ready",
    "desktop-unavailable",
    ...PORTABILITY_DIALOGS.map((d) => `window-${d}`),
    ...IMPORT_FORMATS.flatMap((f) => [importFormatId(f), `portability-import-format-hint-${f}`]),
    ...EXPORT_FORMATS.map(exportFormatId),
    ...EXPORT_SOURCES.map((s) => `portability-export-source-${s}`),
    ...MBOX_VARIANTS.map(mboxVariantId),
    ...JOB_KINDS.map(jobKindId),
    ...JOB_PHASES.map(jobPhaseId),
    ...JOB_STATES.map(jobStateId),
    ...SEVERITIES.map(severityId),
    ...ISSUE_CODES.map(issueCodeId),
    ...BACKUP_ITEMS.flatMap((i) => [backupItemId(i), `portability-backup-item-hint-${i}`]),
    ...["nothing-chosen", "duplicate-item", "passphrase-too-short", "passphrase-too-long"].map(
      (i) => `backup-issue-${i}`,
    ),
    "backup-wrong-passphrase",
    ...RESTORE_EFFECTS.map(restoreEffectId),
    ...DIAGNOSTICS_ITEMS.flatMap((i) => [
      diagnosticsItemId(i),
      `portability-diagnostics-item-hint-${i}`,
    ]),
    ...REDACTIONS.map(redactionId),
    ...["nothing-chosen", "no-messages", "messages-without-mail-item", "too-many-messages"].map(
      (i) => `diagnostics-issue-${i}`,
    ),
    ...["calendar", "task-list", "address-book"].map((k) => `import-collection-${k}`),
    ...COUNT_KEYS.map(countId),
    ...["mbox", "all", "ics", "vcard", "eml", "backup", "zip"].map(
      (f) => `portability-filter-${f}`,
    ),
    // Mock / core texts the UI shows through `localized`.
    "portability-path-missing",
    "portability-export-mismatch",
    "portability-target-exists",
    "portability-job-gone",
    "portability-restore-note-passwords",
  ];
}
