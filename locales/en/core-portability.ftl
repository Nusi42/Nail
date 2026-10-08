# Nail — English texts of import, export, backup / restore and the
# diagnostics bundle built in Rust or named by the contract (M4c; contract
# ADR 0094): formats, job kinds / phases / states, per-item issues, backup
# items, restore effects, diagnostics items and redactions.
#
# The ids named by the contract (nail_api::portability, nail_api::backup,
# nail_api::diagnostics) are pre-filled; nail-api/tests/catalog.rs checks
# they exist. WP121 (nail-portability) adds its own texts below its
# heading; the UI's texts are in portability.ftl (WP124).

## Not served yet (contract stage)

m4c-not-ready = This is not available in this version of Nail yet.

## Formats (ImportFormat, ExportFormat, MboxVariant, ImportCollectionKind)

import-format-mbox = mbox file
import-format-eml-folder = Folder of .eml files
import-format-maildir = Maildir folder
import-format-thunderbird-profile = Thunderbird profile
import-format-ics = Calendar file (.ics)
import-format-vcard = Contacts file (.vcf)
export-format-mbox = { import-format-mbox }
export-format-eml = .eml files
export-format-maildir = { import-format-maildir }
export-format-ics = { import-format-ics }
export-format-vcard = { import-format-vcard }
mbox-variant-mboxrd = mboxrd (recommended)
mbox-variant-mboxo = mboxo (older programs)
import-collection-calendar = { common-calendar }
import-collection-task-list = { common-task-list }
import-collection-address-book = Address book

## Jobs (JobKind, JobPhase, JobState, IssueSeverity)

portability-job-import = { common-import }
portability-job-export = Export
portability-job-backup = Backup
portability-job-verify-backup = Backup check
portability-job-restore = { common-restore }
portability-phase-queued = Waiting
portability-phase-scanning = Counting
portability-phase-reading = Reading
portability-phase-writing = Writing
portability-phase-verifying = Checking
portability-phase-finishing = Finishing
portability-state-running = Running
portability-state-completed = { common-state-done }
portability-state-completed-with-issues = Done, with problems
portability-state-failed = { common-failed }
portability-state-cancelled = { common-cancelled }
portability-severity-warning = Changed
portability-severity-skipped = Left out

## Per-item issues (PortabilityIssueCode)

portability-issue-unreadable-file = The file could not be read.
portability-issue-malformed-message = This is not a valid message.
portability-issue-too-large = The message is too large to import.
portability-issue-truncated = The last message of the file is cut off.
portability-issue-duplicate = Already there.
portability-issue-unknown-flag = An unknown flag was dropped.
portability-issue-invalid-folder-name = The folder name was changed.
portability-issue-invalid-calendar-data = The calendar data could not be read.
portability-issue-invalid-contact-data = The contact data could not be read.
portability-issue-unsupported-format = This variant of the format cannot be read.
portability-issue-filter-not-expressible = Part of this filter has no equivalent in Nail rules.
portability-issue-account-not-expressible = An account setting has no equivalent in Nail.
portability-issue-secret-not-imported = Passwords are never imported: sign in again after adding the account.
portability-issue-write-failed = Writing failed.
portability-issue-target-exists = A file with this name exists and was not replaced.
portability-issue-checksum-mismatch = This part of the backup is damaged.
portability-issue-entry-missing = This part of the backup is missing.
portability-issue-newer-format = This backup was made by a newer version of Nail.
portability-issue-needs-attention = This needs your attention after the restore.

## Backups (BackupItem, BackupIssue, RestoreEffect)

backup-item-settings = { common-settings }
backup-item-theme = Theme
backup-item-layout = { common-layout }
backup-item-rules = Rules and saved searches
backup-item-accounts = Accounts (without passwords)
backup-item-local-mail = Local folders
backup-item-keys = Encryption keys
backup-item-pim-local = Local calendars, contacts and tasks
backup-item-plugins = { common-plug-ins }
backup-issue-nothing-chosen = Choose at least one thing to back up.
backup-issue-duplicate-item = An item is listed twice.
backup-issue-passphrase-too-short = The passphrase must have at least 12 characters.
backup-issue-passphrase-too-long = The passphrase is too long.
backup-wrong-passphrase = The passphrase is wrong, or this is not a Nail backup.
restore-effect-replace = Replace
restore-effect-merge = Add what is missing
restore-effect-add = { common-add }
restore-effect-skip = Leave as is

## Diagnostics (DiagnosticsItem, Redaction, DiagnosticsIssue)

diagnostics-item-system-info = System information
diagnostics-item-logs = Recent logs
diagnostics-item-config = Configuration files
diagnostics-item-account-status = Account status
diagnostics-item-store-stats = Database statistics
diagnostics-item-operation-queue = Pending changes
diagnostics-item-plugins = { common-plug-ins }
diagnostics-item-mail-headers = Headers of chosen messages
diagnostics-item-mail-content = Chosen messages (full content)
diagnostics-redaction-secrets = Passwords and keys removed
diagnostics-redaction-addresses = Mail addresses replaced
diagnostics-redaction-subjects = Subjects and names removed
diagnostics-redaction-paths = Your user name in paths replaced
diagnostics-issue-nothing-chosen = Choose at least one thing to include.
diagnostics-issue-no-messages = Choose the messages to include.
diagnostics-issue-messages-without-mail-item = Messages are only included with “Headers” or “Full content”.
diagnostics-issue-too-many-messages = Include at most 20 messages.

## Importers, exporters, backups (WP121)

# Why a job stopped (PortabilityError::localized).
portability-error-io = A file could not be read or written.
portability-error-invalid-source = This is not a valid source for the chosen format.
portability-error-target-exists = The target exists. Choose another name or allow replacing it.
portability-error-not-a-backup = This file is not a Nail backup.
portability-error-corrupt-backup = The backup is damaged and cannot be read.
portability-error-invalid-options = These settings cannot be used.
portability-error-store = Nail could not save the imported data.
portability-error-source-changed = Your data changed while the backup was written. Try again.

# Messages deleted in the source program (PortabilityIssueCode::DeletedInSource).
portability-issue-deleted-in-source = Deleted in the other program and not imported.

# Notes of a restore plan or report (RestoreAction.note).
restore-note-accounts-need-passwords = { $count ->
    [one] One account needs its password again.
   *[other] { $count } accounts need their passwords again.
}
restore-note-plugins-review = Check the plug-ins' permissions after the restore.

## The core (WP125)

portability-issue-folder-merged = This folder does not exist in the account; its messages went into the chosen folder.
portability-error-account-folders = Nail creates new folders only in Local Folders. To import into this account, choose one of its existing folders.
portability-error-virtual-target = Mail cannot be imported into a combined account. Choose one of its accounts.
portability-error-path-not-absolute = Type the full path of the file or folder.
portability-error-missing-path = { common-path-missing }
portability-error-format-mismatch = This format cannot export what you chose.
portability-error-nothing-chosen = Choose what to export.
portability-job-unknown = This job is not known (any more).
local-folders-account = Local Folders
portability-export-messages-folder = { common-messages }
