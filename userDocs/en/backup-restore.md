# Backups and restoring

A Nail backup is one file (`.nailbackup`) that holds your settings and
local data, encrypted with a passphrase you choose. Use it to move Nail to
another computer or to get back to a known state.

Back up and restore in **Settings → Backups** (**Back up…**,
**Restore…**) or from the [command palette](command-palette.md). Backups
are only available in the desktop app and in the local browser window, not
in remote sessions.

## What a backup can hold

| Item | What it is | In new backups |
| --- | --- | --- |
| Settings | Nail's settings (security, notifications, sync, desktop and update settings, recent commands) | yes |
| Theme | your `theme.kdl` | yes |
| Layout | `layout.kdl` | yes |
| Rules and saved searches | `rules.kdl` and your saved searches | yes |
| Accounts (without passwords) | `accounts.kdl`: accounts, identities, signatures, calendar and contact sources | yes |
| Local folders | the mail of *Local Folders* and of opened files, with read state, flags and tags | yes |
| Local calendars, contacts and tasks | the calendars, task lists and address books kept on this computer | yes |
| Plug-ins | the packages of your installed plug-ins | yes |
| Encryption keys | your OpenPGP and S/MIME keyring **including secret keys** (a key protected by the Credential Manager takes its protection secret along, so it works on another computer; keys on smartcards stay on the card) | **no** — choose it yourself |

Never in a backup: account passwords and sign-in tokens (they stay in the
Windows Credential Manager; `accounts.kdl` only refers to them), remote
access users, AI agent (MCP) clients and pairing codes. Mail of IMAP,
Gmail and Exchange accounts is not backed up either — it is on the server
and Nail downloads it again.

## The passphrase and the encryption

The passphrase must have at least **12 characters**. Nail cannot recover a
forgotten passphrase — without it the backup cannot be opened. Write it
down somewhere safe, especially if the backup includes your secret keys.

The whole backup is encrypted: XChaCha20-Poly1305 with a key derived from
your passphrase with Argon2id (64 MB of memory, so guessing passphrases is
slow). Even the list of files inside is encrypted. Every part of the file
is authenticated: a damaged or tampered backup is detected, never
silently restored. A wrong passphrase is reported straight away.

## Making a backup

Choose the items, the file and the passphrase. Nail writes the backup
under a temporary name, then opens it again and checks every part before
the job reports success; the report lists what the backup holds (number of
messages, events, contacts, files) and its size.

Nail reads your data twice while writing (once to list it with checksums
at the start of the backup, once to write it). If something changes in
between — for example an import running into Local Folders — the backup
stops and asks you to try again, rather than writing a backup that would
not check out.

## Checking a backup

**Check this backup** (after making one) or **Check the backup first**
(when restoring) reads every part of a backup and compares it with the
checksum recorded when it was made. Do this for backups you keep for a
long time or copied to another drive. The report names each damaged or
missing part.

## Restoring

Restoring always starts with a **dry run**: Nail opens the backup and
shows, for each item, what would happen, without changing anything:

| Shown as | Meaning |
| --- | --- |
| Replace | your current file or settings are replaced; Nail keeps the current version as a configuration backup, so you can go back (the same way as for any other change of a configuration file) |
| Add what is missing | mail, calendar and contact items, keys and plug-ins are merged: what is missing is added, nothing is deleted |
| Add | there is nothing yet; everything is added |
| Leave as is | not chosen, not in the backup, or already the same |

In the restore assistant: open the backup with its passphrase, optionally
check it, choose the items, then **Show what would change** (the dry run)
and **Restore now**. Details:

* **Configuration** (settings, theme, layout, rules, accounts) is replaced
  file by file; files that are already identical are left alone.
* **Local folders** are merged: messages already there (same
  `Message-ID`, date and size) are skipped, so restoring twice does not
  duplicate anything. Read state, flags and tags come back.
* **Calendars, tasks and contacts** are merged: items whose `UID` exists
  are updated, the others added.
* **Accounts need their passwords again**: after restoring accounts, sign
  in to each of them (the report lists them).
* **Plug-ins** that are not installed are installed from the backup
  **switched off and without permissions**: turn them on in Settings →
  Plug-ins and grant what they ask for. Plug-ins already installed stay as
  they are.
* **Restart** Nail when the report says so (after restoring settings or
  plug-ins).
* Damaged parts of a backup are listed and left out; the rest is restored.

A backup made by a newer version of Nail cannot be restored by an older
one; update Nail first.

## See also

* [Importing and exporting](import-export.md) — single folders, calendars
  and address books in standard formats
* [Data protection in detail](data-protection.md) — encryption at rest and
  the recovery key of your local data
