# Nail — English UI strings: import, export and backup wizards, their
# progress and reports, the diagnostics bundle (M4c WP124; contract
# ADR 0094).
#
# Owned by WP124; message ids start with the prefixes listed in
# ui/src/features/portability/messages.ts. Texts the contract names
# (formats, job states, issues, backup items, diagnostics items) are in
# core-portability.ftl.

portability-title = { portability-import-export-title }

## Window titles

window-import-wizard = Import mail, calendars and contacts
window-export-wizard = { portability-job-export }
window-backup-wizard = Back up Nail
window-restore-wizard = Restore from a backup
window-diagnostics-bundle = { portability-diagnostics-title }
window-portability-report = Report

## Command palette

portability-action-import = Import from another program or file…
portability-action-import-keywords = thunderbird mbox eml maildir ics vcf vcard calendar contacts migrate
portability-action-export = Export mail, calendars or contacts…
portability-action-export-keywords = mbox eml maildir ics vcf vcard save archive
portability-action-backup = Back up Nail…
portability-action-backup-keywords = backup archive save settings nailbackup
portability-action-restore = Restore from a backup…
portability-action-restore-keywords = restore backup nailbackup recover
portability-action-diagnostics = Create a diagnostics bundle…
portability-action-diagnostics-keywords = support logs bug report problem debug

## Shared

portability-step = Step { $step } of { $steps }
portability-reading = Reading…
portability-browse = Browse…
portability-browse-folder = Choose folder…
portability-path-hint = Type the full path or browse.
portability-path-type-hint = Type the full path of the file or folder on the computer running Nail.
portability-path-not-absolute = Type the full path, for example C:\Users\you\Documents\mail.mbox.
portability-path-missing = { common-path-missing }
portability-account-option = { $hasEmail ->
    [yes] { $name } ({ $email })
   *[no] { $name }
}
portability-filter-mbox = mbox files
portability-filter-all = All files
portability-filter-ics = Calendar files
portability-filter-vcard = Contact files
portability-filter-eml = Message files
portability-filter-backup = Nail backups
portability-filter-zip = Zip files

## Jobs: progress and reports

portability-progress-phase = { $kind }: { $phase }
portability-progress-bytes = { $done } of { $total }
portability-progress-current = Working on { $name }
portability-cancel-hint = What is already done stays; the report says what.
portability-report-title = { $kind }: { $state }
portability-report-when = Started { $started }, finished { $finished }
portability-job-failed = The job stopped with an error.
portability-job-gone = This job is no longer known. Finished jobs are kept for a day.
portability-counts-none = Nothing was changed.
portability-count-messages = { common-message-count }
portability-count-folders = { $count ->
    [one] 1 folder
   *[other] { $count } folders
}
portability-count-events = { $count ->
    [one] 1 event
   *[other] { $count } events
}
portability-count-tasks = { $count ->
    [one] 1 task
   *[other] { $count } tasks
}
portability-count-contacts = { $count ->
    [one] 1 contact
   *[other] { $count } contacts
}
portability-count-groups = { $count ->
    [one] 1 contact group
   *[other] { $count } contact groups
}
portability-count-accounts = { $count ->
    [one] 1 account
   *[other] { $count } accounts
}
portability-count-rules = { $count ->
    [one] 1 rule
   *[other] { $count } rules
}
portability-count-files = { $count ->
    [one] 1 file
   *[other] { $count } files
}
portability-count-duplicates = { $count ->
    [one] 1 already there (left out)
   *[other] { $count } already there (left out)
}
portability-count-skipped = { $count ->
    [one] 1 left out because of a problem
   *[other] { $count } left out because of problems
}
portability-issues-title = { common-problems }
portability-preview-issues-title = Problems found so far
portability-issues-truncated = { $count ->
    [one] 1 more problem is not listed.
   *[other] { $count } more problems are not listed.
}
portability-issue-details = { itip-label-description }
portability-issue-index = item { $index }
portability-jobs-import-export = Recent imports and exports
portability-jobs-backup = Recent backups and restores
portability-jobs-none = Nothing imported, exported or backed up recently.
portability-jobs-unavailable = Jobs are only available in the desktop app.
portability-job-show-report = Show report
portability-job-show-progress = Show progress

## Import

portability-import-source-title = Where from?
portability-import-intro = Nail reads the source first and shows what it found. Nothing is written until you start the import, and the source is never changed.
portability-import-found = Found on this computer
portability-import-found-profile = Thunderbird profile “{ $name }”
portability-import-found-default = default
portability-import-found-used = last used { $when }
portability-import-found-use = Use this profile
portability-import-format = What do you want to import?
portability-import-format-hint-mbox = One file with many messages (Thunderbird, Apple Mail, Evolution, mutt).
portability-import-format-hint-eml-folder = A folder of single messages saved as .eml files; subfolders become folders.
portability-import-format-hint-maildir = A Maildir tree with cur, new and tmp (Dovecot, Courier, many Linux programs); flags are kept.
portability-import-format-hint-thunderbird-profile = Local folders, address books, and your accounts and filters as suggestions.
portability-import-format-hint-ics = Events and tasks from an iCalendar file.
portability-import-format-hint-vcard = Contacts and groups from a vCard file.
portability-import-file = { common-file }
portability-import-folder = { common-disk-folder }
portability-import-pick-title = Choose what to import
portability-import-read = Read source
portability-import-options-title = What and where
portability-import-preview-intro = Nail found the following in this { $format }. Choose what to import and where it goes.
portability-import-folder-size = { $count ->
    [one] 1 message, { $size }
   *[other] { $count } messages, { $size }
}
portability-import-collection-size = { $kind }, { $count ->
    [one] 1 item
   *[other] { $count } items
}
portability-import-target = Where the mail goes
portability-import-target-local = Local Folders (on this computer only)
portability-import-target-local-hint = Local Folders are never synchronized with a server.
portability-import-target-account-hint = The messages go into an existing folder of the account (a source folder with the same name below it, else the chosen folder) and are uploaded to the server like any change, one by one through the account’s queue. Large imports take a while. New folders are only created in Local Folders.
portability-import-target-parent = Into the folder
portability-import-target-inbox = The Inbox
portability-import-target-top = The account’s top level
portability-import-folder-name = New folder for the import
portability-import-folder-name-hint = Leave empty to put the folders directly there; folders with the same name are merged.
portability-import-folder-default = Import from { $name }
portability-import-skip-duplicates = Leave out messages that are already there
portability-import-skip-duplicates-hint = Same Message-ID, date and size. An interrupted import can then simply be started again.
portability-import-keep-flags = Keep read, flagged and answered marks
portability-import-calendar = Events and tasks go to
portability-import-address-book = Contacts go to
portability-import-default-address-book = The default address book
portability-import-proposals-later = { $accounts ->
    [0] { $rules ->
        [one] 1 filter is offered after the import; nothing is added without you.
       *[other] { $rules } filters are offered after the import; nothing is added without you.
    }
    [one] 1 account and { $rules } filters are offered after the import; nothing is added without you.
   *[other] { $accounts } accounts and { $rules } filters are offered after the import; nothing is added without you.
}
portability-import-job-title = Importing

## Thunderbird proposals

portability-proposals-accounts = Accounts found in Thunderbird
portability-proposals-accounts-hint = Passwords are never imported: after adding an account, sign in or enter its password in the account settings.
portability-proposals-add-accounts = Add chosen accounts
portability-proposals-rules = Filters found in Thunderbird
portability-proposals-save-rules = Save chosen filters as rules
portability-proposal-duplicate = Already in Nail as “{ $account }”.
portability-proposal-added = Added. Sign in to start syncing.
portability-proposal-saved = Saved as a rule.
portability-proposal-rule-complete = Everything in this filter has a rule equivalent.
portability-proposal-rule-incomplete = Parts of this filter have no rule equivalent; the rule starts switched off so you can check it.
portability-proposal-issues = { common-notes }

## Export

portability-export-title = What to export
portability-export-what = { portability-job-export }
portability-export-source-folders = Mail folders
portability-export-source-messages = The selected message
portability-export-source-calendars = { common-calendars }
portability-export-source-address-books = { portability-export-address-books }
portability-export-subfolders = Include subfolders
portability-export-selected-message = The message selected in the message list is exported.
portability-export-address-books = Address books
portability-export-mbox-variant = mbox variant
portability-export-target-file = Save as
portability-export-target-folder = { portability-import-target-parent }
portability-export-pick-title = Choose where to export
portability-export-default-name = Nail export
portability-export-overwrite = Replace existing files
portability-export-overwrite-hint = Without this, Nail stops when the file exists or the folder is not empty.
portability-export-headers-only = Write messages Nail only has the headers of as they are
portability-export-headers-only-hint = Otherwise their content is downloaded first, which needs the server.
portability-export-start = { portability-job-export }
portability-export-job-title = Exporting
portability-export-mismatch = This format cannot hold what you chose.
portability-target-exists = The file exists or the folder is not empty. Tick “Replace existing files” or choose another place.

## Backup and restore

portability-backup-title = What to back up
portability-backup-intro = A backup is one encrypted file. Keep it somewhere else than this computer, and keep the passphrase: without it nobody, not even Nail, can open the backup.
portability-backup-items = Include
portability-backup-item-hint-settings = Nail’s settings (never sign-in tokens, remote access users or AI agent clients).
portability-backup-item-hint-theme = Your theme.kdl.
portability-backup-item-hint-layout = Your layout.kdl.
portability-backup-item-hint-rules = rules.kdl and saved searches.
portability-backup-item-hint-accounts = accounts.kdl; passwords stay in the Windows Credential Manager and must be entered again after a restore elsewhere.
portability-backup-item-hint-local-mail = Mail in Local Folders. Mail on servers is not backed up; it is still on the server.
portability-backup-item-hint-keys = Your OpenPGP and S/MIME keys including secret keys. Off by default: anyone with the backup and its passphrase could sign and decrypt as you.
portability-backup-item-hint-pim-local = Calendars, contacts and tasks kept only in Nail.
portability-backup-item-hint-plugins = Installed plug-ins with their settings and data.
portability-backup-keys-warning = The backup will contain your secret keys. Use a strong passphrase and keep the file safe.
portability-backup-passphrase-hint = At least 12 characters. Nail does not store it.
portability-backup-passphrase-repeat = Passphrase again
portability-backup-passphrase-mismatch = The two passphrases are different.
portability-backup-file = Backup file
portability-backup-pick-title = Save the backup as
portability-backup-default-name = Nail backup { $date }
portability-backup-start = Back up
portability-backup-job-title = Backing up
portability-backup-verify = Check this backup
portability-verify-title = { portability-job-verify-backup }
portability-backup-restore-title = Backup and restore
portability-backup-restore-text = Back up your settings, local mail and local calendars into one encrypted file, check a backup, or restore one.
portability-manifest-title = In this backup
portability-manifest-created = Made { $when } by Nail { $version }
portability-manifest-item = { $item }: { $count ->
    [one] 1 file
   *[other] { $count } files
}, { $size }
portability-manifest-accounts = { $count ->
    [one] 1 account needs its password again after a restore on another computer.
   *[other] { $count } accounts need their passwords again after a restore on another computer.
}
portability-restore-open-title = Open a backup
portability-restore-intro = Nail first shows what the backup contains and exactly what a restore would change. Nothing changes before you confirm.
portability-restore-file = { portability-backup-file }
portability-restore-pick-title = Choose a backup
portability-restore-choose-title = Choose what to restore
portability-restore-verify = Check the backup first
portability-restore-dry-run = Show what would change
portability-restore-plan-title = What the restore would do
portability-restore-plan-intro = This is a dry run: nothing has changed yet.
portability-restore-plan-caption = Planned changes
portability-restore-plan-issues = Damaged parts (left out)
portability-plan-item = Item
portability-plan-effect = Change
portability-plan-what = Entries
portability-plan-note = { common-note }
portability-plan-numbers = { $added } added, { $replaced } replaced, { $unchanged } unchanged
portability-restore-note-passwords = { $count ->
    [one] 1 account needs its password
   *[other] { $count } accounts need their passwords
}
portability-restore-passwords = Enter the passwords of these accounts after the restore: { $accounts }
portability-restore-restart-planned = Nail must restart after this restore.
portability-restore-nothing-deleted = Nothing is deleted. Replaced configuration files are kept as backups you can return to under Customize.
portability-restore-start = Restore now
portability-restore-job-title = Restoring
portability-restore-report-caption = What was restored
portability-restore-config-backups = { $count ->
    [one] 1 replaced configuration file was kept as a backup.
   *[other] { $count } replaced configuration files were kept as backups.
}
portability-restore-restart = Restart Nail to finish the restore.

## Settings widgets

portability-settings-tab-import-export = { portability-import-export-title }
portability-settings-tab-backup = Backups
portability-settings-tab-diagnostics = Diagnostics
portability-import-export-title = Import and export
portability-import-export-text = Bring in mail, calendars and contacts from other programs and files, or write them out.
portability-open-import = { common-import-open }
portability-open-export = { common-export-ellipsis }
portability-open-backup = Back up…
portability-open-restore = Restore…

## Diagnostics bundle

portability-diagnostics-title = Diagnostics for support
portability-diagnostics-widget-text = If support asks for it, Nail writes a zip with logs and settings. You see exactly what goes in before it is saved, and nothing is sent anywhere.
portability-diagnostics-open = Create diagnostics bundle…
portability-diagnostics-intro = Choose what to include. Passwords, tokens and keys are always removed. Mail content is only included for messages you pick.
portability-diagnostics-item-hint-system-info = Nail and Windows versions, WebView2, settings that affect behaviour.
portability-diagnostics-item-hint-logs = The recent log files.
portability-diagnostics-item-hint-config = accounts.kdl, layout.kdl, theme.kdl, rules.kdl without secrets.
portability-diagnostics-item-hint-account-status = Connection states and the last errors per account.
portability-diagnostics-item-hint-store-stats = Numbers of messages and folders, database size and version.
portability-diagnostics-item-hint-operation-queue = Changes waiting for the server: kinds and errors, no content.
portability-diagnostics-item-hint-plugins = Installed plug-ins, versions and recent errors.
portability-diagnostics-item-hint-mail-headers = The headers of messages you pick below.
portability-diagnostics-item-hint-mail-content = The complete messages you pick below, attachments included.
portability-diagnostics-messages = Messages to include
portability-diagnostics-mail-warning = These messages go into the bundle unchanged, with their addresses and content. At most { $max } messages.
portability-diagnostics-select-message = Select a message in the message list first; it is offered here.
portability-diagnostics-message = { $subject }
portability-diagnostics-message-loading = Loading the message…
portability-diagnostics-redact = Replace mail addresses and remove subjects
portability-diagnostics-redact-hint = In logs, settings and account states. Support can still follow what happened.
portability-diagnostics-secrets = Passwords, sign-in tokens and keys are always removed.
portability-diagnostics-preview = Show what will be included
portability-diagnostics-preview-title = The bundle will contain exactly these files
portability-diagnostics-preview-summary = { $count ->
    [one] 1 file, { $size }
   *[other] { $count } files, { $size }
}
portability-diagnostics-manifest = List of the contents
portability-diagnostics-redactions = Removed
portability-diagnostics-show-text = Show text
portability-diagnostics-file = Save the bundle as
portability-diagnostics-pick-title = Save the diagnostics bundle as
portability-diagnostics-default-name = Nail diagnostics { $date }
portability-diagnostics-save = Save bundle
portability-diagnostics-sha256 = Checksum (SHA-256): { $hash }
portability-diagnostics-saved-hint = Send the file to support yourself. Nail does not upload it.
