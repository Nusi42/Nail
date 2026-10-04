/**
 * Feature slot of M4c WP124 (contract ADR 0094): import (mbox, EML folders,
 * Maildir, Thunderbird profiles with account / filter proposals, ICS,
 * vCard), export, backup / verify / restore (dry run first) as wizards with
 * progress and per-item reports, and the diagnostics bundle (user-initiated,
 * shows exactly what is included, no mail content by default).
 *
 * Contributions: widgets `import-export`, `backup-restore`, `diagnostics`
 * (future widgets of `nail-config/src/widgets/portability.rs`); dialogs
 * `import-wizard`, `export-wizard`, `backup-wizard`, `restore-wizard`,
 * `diagnostics-bundle`, `portability-report`; actions `import.open` (the
 * first-run screen's import offer), `export.open`, `backup.create`,
 * `backup.restore`, `diagnostics.open` with their palette descriptions;
 * `PathField` (typed path or the system file dialog) for the other M4c
 * slots; theme base CSS `PORTABILITY_BASE_CSS`.
 *
 * Only WP124 edits the files in this directory (plus
 * `locales/{en,de}/portability.ftl`). The central registry
 * (`../registry.ts`) merges these exports into the app; see `../README.md`.
 * The contract's texts are in `locales/en/core-portability.ftl`.
 */
import type { ActionInfo } from "../../actions/palette";
import type { Event as NailEvent } from "../../api/generated";
import type { Feature, FeatureEventHandler } from "../types";
import { BackupWizardDialog, RestoreWizardDialog } from "./BackupWizard";
import { DiagnosticsDialog, DiagnosticsWidget } from "./Diagnostics";
import { ExportWizardDialog } from "./ExportWizard";
import { ImportWizardDialog } from "./ImportWizard";
import {
  BACKUP_ACTION,
  BACKUP_DIALOG,
  BACKUP_RESTORE_WIDGET,
  DIAGNOSTICS_ACTION,
  DIAGNOSTICS_DIALOG,
  DIAGNOSTICS_WIDGET,
  EXPORT_ACTION,
  EXPORT_DIALOG,
  IMPORT_ACTION,
  IMPORT_DIALOG,
  IMPORT_EXPORT_WIDGET,
  REPORT_DIALOG,
  RESTORE_ACTION,
  RESTORE_DIALOG,
} from "./names";
import { applyJobEvent, portabilityKeys } from "./queries";
import { portabilityStoreFor } from "./store";
import { BackupRestoreWidget, ImportExportWidget, ReportDialog } from "./Widgets";

export * from "./names";
export { portabilityKeys } from "./queries";
export { portabilityStoreFor } from "./store";
export { PathField } from "./PathField";
export { PORTABILITY_BASE_CSS } from "./parts";

/** The core's events that change what this feature shows. */
export const PORTABILITY_EVENTS = [
  "PortabilityJobProgress",
  "PortabilityJobFinished",
] as const satisfies readonly NailEvent["type"][];

/** Layout widget kinds → components (kinds registered in nail-config). */
export const widgets: Feature["widgets"] = {
  [IMPORT_EXPORT_WIDGET]: ImportExportWidget,
  [BACKUP_RESTORE_WIDGET]: BackupRestoreWidget,
  [DIAGNOSTICS_WIDGET]: DiagnosticsWidget,
};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  /** Opens the import wizard (also offered by the first-run screen). */
  [IMPORT_ACTION]: {
    run: (ctx) => {
      ctx.store.getState().openWindow(IMPORT_DIALOG);
    },
  },
  /** Opens the export wizard (the current folder / message preselected). */
  [EXPORT_ACTION]: {
    run: (ctx) => {
      ctx.store.getState().openWindow(EXPORT_DIALOG);
    },
  },
  /** Opens the backup wizard. */
  [BACKUP_ACTION]: {
    run: (ctx) => {
      ctx.store.getState().openWindow(BACKUP_DIALOG);
    },
  },
  /** Opens the restore wizard. */
  [RESTORE_ACTION]: {
    run: (ctx) => {
      ctx.store.getState().openWindow(RESTORE_DIALOG);
    },
  },
  /**
   * Opens the diagnostics bundle dialog; the selected message is offered
   * (never included unless ticked).
   */
  [DIAGNOSTICS_ACTION]: {
    run: (ctx) => {
      const selected = ctx.store.getState().selectedMessageId;
      portabilityStoreFor(ctx.store)
        .getState()
        .setDiagnosticsMessages(selected === null ? [] : [selected]);
      ctx.store.getState().openWindow(DIAGNOSTICS_DIALOG);
    },
  },
};

/** Command-palette descriptions (`portability.ftl`). */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
  [IMPORT_ACTION]: {
    title: "portability-action-import",
    keywords: "portability-action-import-keywords",
  },
  [EXPORT_ACTION]: {
    title: "portability-action-export",
    keywords: "portability-action-export-keywords",
  },
  [BACKUP_ACTION]: {
    title: "portability-action-backup",
    keywords: "portability-action-backup-keywords",
  },
  [RESTORE_ACTION]: {
    title: "portability-action-restore",
    keywords: "portability-action-restore-keywords",
  },
  [DIAGNOSTICS_ACTION]: {
    title: "portability-action-diagnostics",
    keywords: "portability-action-diagnostics-keywords",
  },
};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {
  [IMPORT_DIALOG]: ImportWizardDialog,
  [EXPORT_DIALOG]: ExportWizardDialog,
  [BACKUP_DIALOG]: BackupWizardDialog,
  [RESTORE_DIALOG]: RestoreWizardDialog,
  [DIAGNOSTICS_DIALOG]: DiagnosticsDialog,
  [REPORT_DIALOG]: ReportDialog,
};

/**
 * The window model of this feature's windows (M5c, ADR 0116): tool windows
 * are non-modal with their default and minimum sizes; only confirmations
 * (`confirmation: true`) are modal. The layout's `window` properties win.
 */
export const windows: NonNullable<Feature["windows"]> = {
  [IMPORT_DIALOG]: { size: { width: 760, height: 600 }, min_size: { width: 560, height: 440 } },
  [EXPORT_DIALOG]: { size: { width: 760, height: 600 }, min_size: { width: 560, height: 440 } },
  [BACKUP_DIALOG]: { size: { width: 640, height: 520 }, min_size: { width: 480, height: 380 } },
  [RESTORE_DIALOG]: { size: { width: 640, height: 520 }, min_size: { width: 480, height: 380 } },
  [DIAGNOSTICS_DIALOG]: {
    size: { width: 640, height: 520 },
    min_size: { width: 480, height: 380 },
  },
  [REPORT_DIALOG]: { size: { width: 720, height: 560 }, min_size: { width: 480, height: 380 } },
};

/** Keeps jobs current: progress and reports go into the cache. */
export const handlePortabilityEvent: FeatureEventHandler = (event, { queryClient }) => {
  if ((PORTABILITY_EVENTS as readonly string[]).includes(event.type)) {
    applyJobEvent(queryClient, event);
    void queryClient.invalidateQueries({ queryKey: portabilityKeys.jobs });
  }
};

/** Handlers for the core's events. */
export const events: Feature["events"] = [handlePortabilityEvent];
/** Background components. */
export const effects: Feature["effects"] = [];

/**
 * Settings labels per widget kind, for the settings search (M5b WP163, ADR
 * 0109): Fluent ids of the setting labels each widget shows.
 */
export const settings: NonNullable<Feature["settings"]> = {
  [IMPORT_EXPORT_WIDGET]: {
    labels: [
      "portability-settings-tab-import-export",
      "portability-import-export-title",
      "portability-open-import",
      "portability-open-export",
      "portability-jobs-import-export",
    ],
    keywords: "settings-keywords-import-export",
  },
  [BACKUP_RESTORE_WIDGET]: {
    labels: [
      "portability-settings-tab-backup",
      "portability-backup-restore-title",
      "portability-open-backup",
      "portability-open-restore",
      "portability-jobs-backup",
    ],
  },
  [DIAGNOSTICS_WIDGET]: {
    labels: [
      "portability-settings-tab-diagnostics",
      "portability-diagnostics-title",
      "portability-diagnostics-open",
    ],
    keywords: "settings-keywords-diagnostics",
  },
};
