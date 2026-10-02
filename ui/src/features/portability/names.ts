/**
 * Names this feature registers: layout widget kinds (also in
 * `crates/nail-config/src/widgets/portability.rs`), dialog names (window
 * titles `window-<name>` in `portability.ftl`) and action ids.
 */

/** Widget: import and export (buttons and their recent jobs). */
export const IMPORT_EXPORT_WIDGET = "import-export";
/** Widget: backup, check and restore (buttons and their recent jobs). */
export const BACKUP_RESTORE_WIDGET = "backup-restore";
/** Widget: the diagnostics bundle (explanation and the button). */
export const DIAGNOSTICS_WIDGET = "diagnostics";

/** Dialog: the import wizard. */
export const IMPORT_DIALOG = "import-wizard";
/** Dialog: the export wizard. */
export const EXPORT_DIALOG = "export-wizard";
/** Dialog: the backup wizard. */
export const BACKUP_DIALOG = "backup-wizard";
/** Dialog: the restore wizard (open, check, dry run, restore). */
export const RESTORE_DIALOG = "restore-wizard";
/** Dialog: the diagnostics bundle. */
export const DIAGNOSTICS_DIALOG = "diagnostics-bundle";
/** Dialog: the report of a finished job (from the job lists). */
export const REPORT_DIALOG = "portability-report";

/**
 * Action: opens the import wizard. The first-run screen offers "Import"
 * while an action with this id is registered (`system/names.ts`).
 */
export const IMPORT_ACTION = "import.open";
/** Action: opens the export wizard (with the selected message / folder). */
export const EXPORT_ACTION = "export.open";
/** Action: opens the backup wizard. */
export const BACKUP_ACTION = "backup.create";
/** Action: opens the restore wizard. */
export const RESTORE_ACTION = "backup.restore";
/** Action: opens the diagnostics bundle dialog. */
export const DIAGNOSTICS_ACTION = "diagnostics.open";

export const PORTABILITY_DIALOGS = [
  IMPORT_DIALOG,
  EXPORT_DIALOG,
  BACKUP_DIALOG,
  RESTORE_DIALOG,
  DIAGNOSTICS_DIALOG,
  REPORT_DIALOG,
] as const;
