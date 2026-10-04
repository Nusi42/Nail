/**
 * Pure helpers of the import / export / backup / diagnostics UI that mirror
 * `nail_api::{portability, backup, diagnostics}` (ADR 0094): the formats'
 * shapes (file or folder, mail or PIM data, file-dialog filters), which
 * export formats fit which source, the checks the core makes before a job
 * starts (backup contents, passphrases, diagnostics options), progress
 * fractions and byte sizes.
 */
import type {
  BackupContents,
  BackupIssue,
  BackupItem,
  DiagnosticsIssue,
  DiagnosticsItem,
  DiagnosticsOptions,
  ExportFormat,
  ExportSource,
  ImportFormat,
  IssueSeverity,
  ItemIssue,
  JobCounts,
  JobKind,
  JobPhase,
  JobProgress,
  JobState,
  MboxVariant,
  PathFilter,
  PortabilityIssueCode,
  Redaction,
  RestoreEffect,
} from "../../api/generated";
import { charCount } from "../../text/chars";

export const IMPORT_FORMATS: readonly ImportFormat[] = [
  "mbox",
  "eml-folder",
  "maildir",
  "thunderbird-profile",
  "ics",
  "vcard",
];
export const EXPORT_FORMATS: readonly ExportFormat[] = ["mbox", "eml", "maildir", "ics", "vcard"];
export const MBOX_VARIANTS: readonly MboxVariant[] = ["mboxrd", "mboxo"];
export const JOB_KINDS: readonly JobKind[] = [
  "import",
  "export",
  "backup",
  "verify-backup",
  "restore",
];
export const JOB_PHASES: readonly JobPhase[] = [
  "queued",
  "scanning",
  "reading",
  "writing",
  "verifying",
  "finishing",
];
export const JOB_STATES: readonly JobState[] = [
  "running",
  "completed",
  "completed-with-issues",
  "failed",
  "cancelled",
];
export const SEVERITIES: readonly IssueSeverity[] = ["warning", "skipped"];
export const ISSUE_CODES: readonly PortabilityIssueCode[] = [
  "unreadable-file",
  "malformed-message",
  "too-large",
  "truncated",
  "duplicate",
  "unknown-flag",
  "invalid-folder-name",
  "invalid-calendar-data",
  "invalid-contact-data",
  "unsupported-format",
  "filter-not-expressible",
  "account-not-expressible",
  "secret-not-imported",
  "write-failed",
  "target-exists",
  "checksum-mismatch",
  "entry-missing",
  "newer-format",
  "needs-attention",
];
export const BACKUP_ITEMS: readonly BackupItem[] = [
  "settings",
  "theme",
  "layout",
  "rules",
  "accounts",
  "local-mail",
  "keys",
  "pim-local",
  "plugins",
];
export const RESTORE_EFFECTS: readonly RestoreEffect[] = ["replace", "merge", "add", "skip"];
export const DIAGNOSTICS_ITEMS: readonly DiagnosticsItem[] = [
  "system-info",
  "logs",
  "config",
  "account-status",
  "store-stats",
  "operation-queue",
  "plugins",
  "mail-headers",
  "mail-content",
];
export const REDACTIONS: readonly Redaction[] = ["secrets", "addresses", "subjects", "paths"];

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

/** `nail_api::portability::LOCAL_FOLDERS_ACCOUNT_ID`. */
export const LOCAL_FOLDERS_ACCOUNT_ID = "local-folders";
/** `nail_api::backup::BACKUP_FILE_EXTENSION`. */
export const BACKUP_FILE_EXTENSION = "nailbackup";
/** `nail_api::backup::MIN_BACKUP_PASSPHRASE_CHARS`. */
export const MIN_BACKUP_PASSPHRASE_CHARS = 12;
/** `nail_api::backup::MAX_BACKUP_PASSPHRASE_CHARS`. */
export const MAX_BACKUP_PASSPHRASE_CHARS = 1024;
/** `nail_api::diagnostics::MAX_DIAGNOSTICS_MESSAGES`. */
export const MAX_DIAGNOSTICS_MESSAGES = 20;

/** Fluent ids of the contract's labels (`core-portability.ftl`). */
export const importFormatId = (f: ImportFormat) => `import-format-${f}`;
export const exportFormatId = (f: ExportFormat) => `export-format-${f}`;
export const mboxVariantId = (v: MboxVariant) => `mbox-variant-${v}`;
export const jobKindId = (k: JobKind) => `portability-job-${k}`;
export const jobPhaseId = (p: JobPhase) => `portability-phase-${p}`;
export const jobStateId = (s: JobState) => `portability-state-${s}`;
export const severityId = (s: IssueSeverity) => `portability-severity-${s}`;
export const issueCodeId = (c: PortabilityIssueCode) => `portability-issue-${c}`;
export const backupItemId = (i: BackupItem) => `backup-item-${i}`;
export const backupIssueId = (i: BackupIssue) => `backup-issue-${i}`;
export const restoreEffectId = (e: RestoreEffect) => `restore-effect-${e}`;
export const diagnosticsItemId = (i: DiagnosticsItem) => `diagnostics-item-${i}`;
export const redactionId = (r: Redaction) => `diagnostics-redaction-${r}`;
export const diagnosticsIssueId = (i: DiagnosticsIssue) => `diagnostics-issue-${i}`;

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
  return { items: BACKUP_ITEMS.filter(backupDefaultOn) };
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
    items: DIAGNOSTICS_ITEMS.filter((i) => !diagnosticsIsMail(i)),
    message_ids: [],
    redact_addresses: true,
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

/** `JobProgress::fraction`: 0..1 when the total is known. */
export function progressFraction(progress: JobProgress): number | null {
  if (progress.total !== null && progress.total > 0) {
    return Math.min(1, progress.done / progress.total);
  }
  if (progress.bytes_total !== null && progress.bytes_total > 0) {
    return Math.min(1, progress.bytes_done / progress.bytes_total);
  }
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
