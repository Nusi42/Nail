/**
 * Pure helpers of the import / export / backup / diagnostics UI
 * (`nail_api::{portability, backup, diagnostics}`, ADR 0094): the value
 * lists, Fluent ids, limits and defaults come generated; the formats' shapes
 * (file or folder, mail or PIM data), which export formats fit which source,
 * the checks the core makes before a job starts and progress fractions are
 * ports pinned by `crates/nail-api/tests/ts_vectors.rs` (vectors run in
 * `model.test.ts`); file-dialog filters and byte sizes are the UI's own.
 */
import {
  BACKUP_FILE_EXTENSION,
  BACKUP_ISSUE_MESSAGE_IDS,
  BACKUP_ITEM_MESSAGE_IDS,
  DEFAULT_BACKUP_CONTENTS,
  DEFAULT_DIAGNOSTICS_OPTIONS,
  DIAGNOSTICS_ISSUE_MESSAGE_IDS,
  DIAGNOSTICS_ITEM_MESSAGE_IDS,
  EXPORT_FORMAT_MESSAGE_IDS,
  IMPORT_FORMAT_MESSAGE_IDS,
  ISSUE_SEVERITY_MESSAGE_IDS,
  JOB_KIND_MESSAGE_IDS,
  JOB_PHASE_MESSAGE_IDS,
  JOB_STATE_MESSAGE_IDS,
  MAX_BACKUP_PASSPHRASE_CHARS,
  MAX_DIAGNOSTICS_MESSAGES,
  MBOX_VARIANT_MESSAGE_IDS,
  MIN_BACKUP_PASSPHRASE_CHARS,
  PORTABILITY_ISSUE_CODE_MESSAGE_IDS,
  REDACTION_MESSAGE_IDS,
  RESTORE_EFFECT_MESSAGE_IDS,
  type BackupContents,
  type BackupIssue,
  type BackupItem,
  type DiagnosticsIssue,
  type DiagnosticsItem,
  type DiagnosticsOptions,
  type ExportFormat,
  type ExportSource,
  type ImportFormat,
  type IssueSeverity,
  type ItemIssue,
  type JobCounts,
  type JobKind,
  type JobPhase,
  type JobProgress,
  type JobState,
  type MboxVariant,
  type PathFilter,
  type PortabilityIssueCode,
  type Redaction,
  type RestoreEffect,
} from "../../api/generated";
import { charCount } from "../../text/chars";

export {
  BACKUP_FILE_EXTENSION,
  BACKUP_ITEM_VALUES as BACKUP_ITEMS,
  DIAGNOSTICS_ITEM_VALUES as DIAGNOSTICS_ITEMS,
  EXPORT_FORMAT_VALUES as EXPORT_FORMATS,
  IMPORT_FORMAT_VALUES as IMPORT_FORMATS,
  ISSUE_SEVERITY_VALUES as SEVERITIES,
  JOB_KIND_VALUES as JOB_KINDS,
  JOB_PHASE_VALUES as JOB_PHASES,
  JOB_STATE_VALUES as JOB_STATES,
  LOCAL_FOLDERS_ACCOUNT_ID,
  MAX_BACKUP_PASSPHRASE_CHARS,
  MAX_DIAGNOSTICS_MESSAGES,
  MBOX_VARIANT_VALUES as MBOX_VARIANTS,
  MIN_BACKUP_PASSPHRASE_CHARS,
  PORTABILITY_ISSUE_CODE_VALUES as ISSUE_CODES,
  REDACTION_VALUES as REDACTIONS,
  RESTORE_EFFECT_VALUES as RESTORE_EFFECTS,
} from "../../api/generated";

/** The counts a report shows, in this order (zero counts are left out). */
export const COUNT_KEYS: readonly (keyof JobCounts)[] = [
  "messages",
  "folders",
  "events",
  "tasks",
  "contacts",
  "groups",
  "accounts",
  "rules",
  "files",
  "duplicates",
  "skipped",
];

/** Fluent id of a count (`portability-count-messages`). */
export const countId = (key: keyof JobCounts) => `portability-count-${key}`;

/** Fluent ids of the contract's labels (`core-portability.ftl`). */
export const importFormatId = (f: ImportFormat) => IMPORT_FORMAT_MESSAGE_IDS[f];
export const exportFormatId = (f: ExportFormat) => EXPORT_FORMAT_MESSAGE_IDS[f];
export const mboxVariantId = (v: MboxVariant) => MBOX_VARIANT_MESSAGE_IDS[v];
export const jobKindId = (k: JobKind) => JOB_KIND_MESSAGE_IDS[k];
export const jobPhaseId = (p: JobPhase) => JOB_PHASE_MESSAGE_IDS[p];
export const jobStateId = (s: JobState) => JOB_STATE_MESSAGE_IDS[s];
export const severityId = (s: IssueSeverity) => ISSUE_SEVERITY_MESSAGE_IDS[s];
export const issueCodeId = (c: PortabilityIssueCode) => PORTABILITY_ISSUE_CODE_MESSAGE_IDS[c];
export const backupItemId = (i: BackupItem) => BACKUP_ITEM_MESSAGE_IDS[i];
export const backupIssueId = (i: BackupIssue) => BACKUP_ISSUE_MESSAGE_IDS[i];
export const restoreEffectId = (e: RestoreEffect) => RESTORE_EFFECT_MESSAGE_IDS[e];
export const diagnosticsItemId = (i: DiagnosticsItem) => DIAGNOSTICS_ITEM_MESSAGE_IDS[i];
export const redactionId = (r: Redaction) => REDACTION_MESSAGE_IDS[r];
export const diagnosticsIssueId = (i: DiagnosticsIssue) => DIAGNOSTICS_ISSUE_MESSAGE_IDS[i];

/** `ImportFormat::is_directory`. */
export function importIsDirectory(format: ImportFormat): boolean {
  return format === "eml-folder" || format === "maildir" || format === "thunderbird-profile";
}

/** `ImportFormat::has_mail`. */
export function importHasMail(format: ImportFormat): boolean {
  return format !== "ics" && format !== "vcard";
}

/** `ImportFormat::has_pim`. */
export function importHasPim(format: ImportFormat): boolean {
  return format === "ics" || format === "vcard" || format === "thunderbird-profile";
}

/** `ExportFormat::accepts`. */
export function exportAccepts(format: ExportFormat, source: ExportSource["type"]): boolean {
  switch (source) {
    case "folders":
    case "messages":
      return format === "mbox" || format === "eml" || format === "maildir";
    case "calendars":
      return format === "ics";
    case "address-books":
      return format === "vcard";
  }
}

/** `ExportFormat::is_directory`. */
export function exportIsDirectory(format: ExportFormat): boolean {
  return format === "eml" || format === "maildir";
}

/** `ExportFormat::extension`. */
export function exportExtension(format: ExportFormat): string | null {
  switch (format) {
    case "mbox":
      return "mbox";
    case "eml":
      return "eml";
    case "maildir":
      return null;
    case "ics":
      return "ics";
    case "vcard":
      return "vcf";
  }
}

/** The file dialog filter of an import format (files only). */
export function importFilters(format: ImportFormat): PathFilter[] {
  switch (format) {
    case "mbox":
      return [
        { label_id: "portability-filter-mbox", extensions: ["mbox", "mbx"] },
        { label_id: "portability-filter-all", extensions: [] },
      ];
    case "ics":
      return [{ label_id: "portability-filter-ics", extensions: ["ics", "ical"] }];
    case "vcard":
      return [{ label_id: "portability-filter-vcard", extensions: ["vcf", "vcard"] }];
    default:
      return [];
  }
}

/** The file dialog filter of an export format (single-file targets). */
export function exportFilters(format: ExportFormat): PathFilter[] {
  const ext = exportExtension(format);
  if (ext === null || exportIsDirectory(format)) return [];
  return [
    { label_id: `portability-filter-${format === "vcard" ? "vcard" : ext}`, extensions: [ext] },
  ];
}

export const BACKUP_FILTERS: PathFilter[] = [
  { label_id: "portability-filter-backup", extensions: [BACKUP_FILE_EXTENSION] },
];
export const DIAGNOSTICS_FILTERS: PathFilter[] = [
  { label_id: "portability-filter-zip", extensions: ["zip"] },
];

/** `BackupItem::default_on`. */
export function backupDefaultOn(item: BackupItem): boolean {
  return item !== "keys";
}

/** `BackupContents::default()`: everything but the keys. */
export function defaultBackupContents(): BackupContents {
  return { items: [...DEFAULT_BACKUP_CONTENTS.items] };
}

/** `BackupContents::issues`. */
export function backupContentsIssues(contents: BackupContents): BackupIssue[] {
  const issues: BackupIssue[] = [];
  if (contents.items.length === 0) issues.push("nothing-chosen");
  if (new Set(contents.items).size !== contents.items.length) issues.push("duplicate-item");
  return issues;
}

/** `backup::passphrase_issues` (new backups only). */
export function passphraseIssues(passphrase: string): BackupIssue[] {
  const chars = charCount(passphrase);
  if (chars < MIN_BACKUP_PASSPHRASE_CHARS) return ["passphrase-too-short"];
  if (chars > MAX_BACKUP_PASSPHRASE_CHARS) return ["passphrase-too-long"];
  return [];
}

/** `DiagnosticsItem::is_mail`. */
export function diagnosticsIsMail(item: DiagnosticsItem): boolean {
  return item === "mail-headers" || item === "mail-content";
}

/** `DiagnosticsOptions::default()`: no mail content, addresses redacted. */
export function defaultDiagnosticsOptions(): DiagnosticsOptions {
  return {
    ...DEFAULT_DIAGNOSTICS_OPTIONS,
    items: [...DEFAULT_DIAGNOSTICS_OPTIONS.items],
    message_ids: [...DEFAULT_DIAGNOSTICS_OPTIONS.message_ids],
  };
}

/** `DiagnosticsOptions::issues`. */
export function diagnosticsIssues(options: DiagnosticsOptions): DiagnosticsIssue[] {
  const issues: DiagnosticsIssue[] = [];
  if (options.items.length === 0) issues.push("nothing-chosen");
  const mail = options.items.some(diagnosticsIsMail);
  if (mail && options.message_ids.length === 0) issues.push("no-messages");
  if (!mail && options.message_ids.length > 0) issues.push("messages-without-mail-item");
  if (options.message_ids.length > MAX_DIAGNOSTICS_MESSAGES) issues.push("too-many-messages");
  return issues;
}

/**
 * `JobProgress::fraction`: done as 0..1 (bytes when their total is known,
 * else items; an empty total is done), `null` while the total is unknown.
 */
export function progressFraction(progress: JobProgress): number | null {
  const { bytes_total: bytesTotal, total } = progress;
  if (bytesTotal !== null && bytesTotal > 0) {
    return Math.min(progress.bytes_done, bytesTotal) / bytesTotal;
  }
  if (total !== null && total > 0) return Math.min(progress.done, total) / total;
  if (total === 0 || (bytesTotal === 0 && total === null)) return 1;
  return null;
}

/** `JobState::is_finished`. */
export function isFinished(state: JobState): boolean {
  return state !== "running";
}

/** A byte size in the UI locale (`1.2 MB`). */
export function formatBytes(bytes: number, locale: string): string {
  const units = ["byte", "kilobyte", "megabyte", "gigabyte", "terabyte"] as const;
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit++;
  }
  return new Intl.NumberFormat(locale, {
    style: "unit",
    unit: units[unit],
    unitDisplay: "short",
    maximumFractionDigits: unit === 0 ? 0 : 1,
  }).format(value);
}

/** Where an item issue is, as plain words for the report (`file › folder #3`). */
export function issueLocation(issue: ItemIssue): string {
  const parts: string[] = [];
  if (issue.item.file) parts.push(issue.item.file);
  if (issue.item.folder) parts.push(issue.item.folder);
  return parts.join(" › ");
}

/**
 * Text without Unicode bidi isolation marks (Fluent wraps arguments in
 * them), for file names built from catalog messages.
 */
export function plainText(text: string): string {
  return text.replace(/[⁦-⁩]/g, "");
}

/** The last segment of a Windows or POSIX path. */
export function baseName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  return cut < 0 ? trimmed : trimmed.slice(cut + 1);
}

/** Whether a typed path is absolute (`C:\…`, `\\server\…`, `/…`). */
export function isAbsolutePath(path: string): boolean {
  return /^([a-zA-Z]:[\\/]|\\\\|\/)/.test(path.trim());
}
