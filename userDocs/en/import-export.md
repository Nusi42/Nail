# Importing and exporting

Nail can bring in mail, calendars and contacts from other programs and
write them out again in standard formats. Imports and exports run in the
background with a progress bar; you can keep working, and you can cancel
at any time (what was already imported or written stays). At the end you
get a **report**: what was imported or written, and every item that was
changed or left out, with the reason.

Start an import or export from **Settings → Data & backup**
(**Import…**, **Export…**), from the welcome screen, or from the
[command palette](command-palette.md). In the desktop app **Browse…** opens
the Windows file dialog; in the browser you type the full path.

Paths are paths on the computer running Nail, so importing and exporting
is only available in the desktop app and in the local browser window, not
in remote sessions.

## What Nail can import

| Source | What you choose | What comes in |
| --- | --- | --- |
| **mbox file** | a file (`.mbox`, or a Thunderbird / Apple Mail / Google Takeout mbox) | the messages, as one folder named after the file |
| **Folder of `.eml` files** | a folder | one message per `.eml` file; the folder and every subfolder become folders |
| **Maildir** | the Maildir folder | `Inbox` and every subfolder (Dovecot / Courier `.Folder.Sub` and nested folders), with read, flagged, answered and draft flags and keywords (tags) |
| **Thunderbird profile** | a profile Nail found, or the profile folder | local folders, address books, and *proposals* for accounts and filters (see below) |
| **Calendar file (`.ics`)** | a file | events and tasks |
| **Contacts file (`.vcf`)** | a file | contacts and contact groups |

First Nail shows a **preview**: the folders with their number of messages
(for very large mbox files an estimate, marked as such), the calendars and
address books, and the first problems it found. You choose which folders
and collections to import and where they go:

* **Mail** goes into **Local Folders**, a local account that is never
  synchronized (Nail creates it the first time), or into a folder of one
  of your accounts.
  * Into **Local Folders** the source's folder tree is created. You can
    have Nail create a folder for the import (for example "Import from
    Thunderbird"); otherwise folders with the same name are merged.
  * Into an **account**, Nail uses the account's **existing** folders:
    each source folder goes into the folder with the same name below the
    folder you choose (**Into the folder**, the Inbox by default), else
    into the chosen folder itself; the report lists folders filed that way.
    Nail creates no new folders on your server during an import — create
    them first, or import into Local Folders and move the messages. Each
    message is uploaded like any other change you make, so it works
    offline and is sent to the server when the account is online.
* **Events and tasks** go into the calendar you choose, **contacts** into
  the address book you choose. An item that is already there (same `UID`)
  is updated instead of added twice.

### Repeating an import

*Skip messages that are already there* (on by default) compares each
message's `Message-ID`, date and size with the target folder. An
interrupted import can therefore simply be started again: it continues
where it stopped and does not duplicate anything.

*Keep flags* (on by default) keeps read, flagged, answered and draft state
and keywords from Maildir file names and from the `Status`, `X-Status`,
`X-Mozilla-Status` and `X-Mozilla-Keys` headers mbox programs write.

### Problems Nail reports

| Report entry | What it means |
| --- | --- |
| Left out: not a valid message | the item has no mail headers |
| Left out: too large | the message is larger than 64 MB |
| Left out: cut off | an mbox file ends inside a message's headers (an mbox that ends before a message's declared length is imported and marked as changed) |
| Left out: deleted in the other program | the other program had deleted the message but not yet removed it from the file (Thunderbird does this until a folder is compacted; Maildir marks it `T`) |
| Changed: unknown flag dropped | a Maildir flag or keyword letter Nail does not know |
| Changed: folder name changed | a folder name Nail cannot use (for example one containing `/`) |
| Changed: filed into the chosen folder | importing into an account: the source folder does not exist in the account, so its messages went into the folder you chose |
| Left out / changed: calendar or contact data | details from the calendar or contact reader, e.g. an event without a start |
| Already there | skipped because of *Skip messages that are already there* (not counted as a problem) |

### Notes on mbox files

Nail reads mbox files of any size without loading them into memory. It
understands the common variants: `mboxrd` and `mboxo` (lines starting with
"From " are written as ">From "), and `mboxcl` / `mboxcl2` (with a
`Content-Length` header). One limitation is built into the old `mboxo`
variant itself: a line that really started with ">From " cannot be told
apart from a quoted "From " line, so it loses its `>`.

## Importing from Thunderbird

Nail lists the Thunderbird profiles of your Windows account (from
Thunderbird's `profiles.ini`), the default profile first. Close
Thunderbird before importing so its files are complete.

* **Local folders** — Thunderbird's *Local Folders* and the mail of POP3
  accounts (which exists only on this computer) are imported as folders
  named after the Thunderbird account, e.g. `Local Folders/Archives/2024`.
  Mail of IMAP accounts is not imported: it is on the server, and Nail
  downloads it when you add the account. Thunderbird's summary files
  (`.msf`) are ignored.
* **Address books** — the personal address book, *Collected Addresses*
  and every other local address book, including old `.mab` address books
  from Thunderbird versions before 78. Mailing lists become contact
  groups.
* **Accounts** — each IMAP or POP3 account is shown as a *proposal* with
  its servers, security, identities and signature. Nothing is added until
  you choose it. **Passwords are never imported** — Thunderbird keeps them
  encrypted, and Nail does not read them; sign in or type the password
  when you add the account. Where a setting has no equivalent, the
  proposal says what changes:
  * "STARTTLS if available" and unencrypted connections become STARTTLS,
    because Nail never sends your password without encryption;
  * OAuth2 sign-in is kept for Google and Microsoft accounts; other
    sign-in methods become a password login;
  * signature files are not imported (signature text is).
  An account that already exists in Nail (same server and user name) is
  marked as such. Feed and newsgroup accounts are not supported.
* **Filters** — each Thunderbird filter is shown as a proposed Nail
  [rule](rules.md). Filters that use something Nail rules cannot express
  (changing the priority, "is in my address book", junk settings, moving
  to a folder of another account, reply templates, …) are still proposed
  without that part; the proposal lists exactly what was left out, and
  such rules start switched off so you can check them first. Filters of
  an account apply to that account once you add it.

## Exporting

You can export folders (with or without their subfolders) or selected
messages, calendars and task lists, and address books.

| Format | What Nail writes |
| --- | --- |
| **mbox** | one folder: one `.mbox` file; several folders: a folder with one `.mbox` file per folder (`Inbox.mbox`, `Inbox/Lists.mbox`). Variant `mboxrd` (recommended, reversible) or `mboxo` for older programs |
| **`.eml` files** | one file per message (`00001 Subject.eml`), one folder per folder |
| **Maildir** | a Maildir with `.Sub` folders, flags in the file names and keywords in `dovecot-keywords` (at most 26 keywords per folder) |
| **iCalendar (`.ics`)** | one file per calendar or task list |
| **vCard (`.vcf`)** | one file per address book |

Nail does not replace existing files unless you allow it; a target folder
must be empty or new. Files are written under a temporary name and only
get their real name when they are complete, so a cancelled or failed
export never leaves a half-written file. mbox exports are read back and
checked at the end.

An exported folder imported again gives the same messages, byte for byte
— except that mbox adds a line end to a message that did not end with
one. Read state and flags are kept by Maildir; mbox files do not carry
them (Nail does not change the messages to add `Status` headers).

## See also

* [Backups and restoring](backup-restore.md)
* [Calendar, contacts and tasks](calendar-contacts-tasks.md) — importing
  a single `.ics` / `.vcf` file into a calendar or address book
* [Filter rules](rules.md)
