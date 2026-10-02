# Recovery key and full-database encryption

This page explains how Nail encrypts its whole database when
**encryption at rest** is on, and how a **recovery key** gets your data
back if the key in the Windows Credential Manager is lost. For what
encryption at rest covers in general, see
[Data protection in detail](data-protection.md).

## The whole database is encrypted

When you turn encryption at rest on, Nail encrypts the database file
itself, page by page (ChaCha20-Poly1305), in addition to the stored
messages, attachments and the search index. Subjects, sender and
recipient addresses, dates, folder names and collected addresses are then
unreadable without the key, too. Nail opens the database with the key
from the Windows Credential Manager when it starts; nothing changes in
how you use it.

Turning encryption at rest on or off rewrites the database file once.
Nail writes the new file next to the old one, checks it, and only then
replaces the old one, so a crash or a power loss at any moment leaves you
with either the old or the new file — never a broken one. The conversion
continues on the next start.

If you turned encryption at rest on with an earlier version of Nail, the
database file itself is encrypted once in the background after the
update.

Next to the database Nail keeps a small file, `vault.json`. It tells Nail
which Credential Manager entry holds the key and contains the key
protected by your recovery key. It contains no readable secret. Do not
delete it.

## Why you want a recovery key

The key that encrypts your data lives in the Windows Credential Manager
of your Windows account. If that entry disappears — a new Windows
profile, a reinstalled system, a reset Credential Manager, or the data
folder copied to another computer — your encrypted data cannot be opened.
A recovery key is a second way in that only you hold.

## Creating the recovery key

In **Settings → Recovery key**, choose **Create recovery key**. Nail shows it
**once**, as 56 characters in groups of four, for example:

```
K7Q2-M4XA-…-9TPW
```

Copy it, **Print** it, or let Nail save a recovery kit text file —
then move that file somewhere safe (a password manager, a USB stick in a
drawer) and away from this computer. Nail never stores the recovery key
itself, so it cannot show it again.

To finish, Nail asks you to type the key back from your copy, so you know
the copy is right. Creating a new recovery key replaces the old one: the
old key stops working. Do this if you think someone saw your recovery key.
Nail saves a recovery kit only as a new file and never over an existing
one; if the kit cannot be saved, nothing changes and your old recovery key
keeps working.

## Checking the recovery key

You can check at any time that the recovery key you kept still works:
type it in the security settings. Nail tells you whether it opens your
data and remembers when you last checked it. Typing mistakes are
forgiven where possible: upper or lower case, spaces or missing dashes,
and the letters `O`, `I` and `L` for the digits `0` and `1`. The last four
characters are a check, so most typos are caught before anything is
tried.

## Restoring access with the recovery key

If Nail starts and cannot find the key, it says that your data is
encrypted and its key is missing, and opens the dialog **Open your data
with the recovery key** by itself instead of showing your folders. Type
the recovery key and choose **Open with recovery key**: Nail puts the key
back into the Windows Credential Manager. Because the whole database
could not be opened, Nail then asks you to **restart it**; after the
restart your data is there as before. (A wrong key is refused and nothing
changes; you can try again.) If the key in
the Credential Manager is there but wrong (for example after copying a
data folder between computers), the same restore puts the right key in.

Without a recovery key there is no way back: mail that is still on your
mail server is downloaded again into a new data folder, but local drafts,
local folders and mail fetched with POP3 and deleted from the server are
lost. Keep backups as well.

## Removing the recovery key

You can remove the recovery key in **Settings → Recovery key**. Turning
encryption at rest off removes it automatically, because the key it
protects no longer exists.
