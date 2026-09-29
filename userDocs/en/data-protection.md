# Data protection in detail

This page explains exactly what **encryption at rest** protects, what the
**sender check** in the reader means, and how Nail decides that a link or
an attachment deserves a warning. For where to switch things on, see
[Signing, encryption and data protection](security.md#data-protection).

## Encryption at rest

Encryption at rest protects the mail Nail keeps on your computer if
someone copies the disk or steals the laptop. It is **off by default for
now; we recommend turning it on** — together with BitLocker if your
Windows edition has it.

### What is encrypted

| Data | Encrypted |
| --- | --- |
| Stored messages and attachments (the full mail) | yes |
| The search index | yes |
| Drafts (text, recipients, attachment list) | yes |
| The preview line of each message in the list | yes |
| Subjects, sender and recipient addresses, dates, flags, folder names | **no** |
| Collected addresses (for suggestions while writing) | **no** |
| Your secret OpenPGP keys and S/MIME private keys | always, whether encryption at rest is on or not |

Subjects, addresses and dates stay readable in the database because Nail
sorts, threads and filters by them. Someone with the disk could see *who*
wrote to you *when* and about *which subject*, but not what the messages
say.

### The key

Nail creates a random key when you turn encryption at rest on and keeps
it in the **Windows Credential Manager** of your Windows account — never
in a file. Anyone who can sign in as you can therefore read your mail in
Nail, as before; the encryption protects against the disk being read
elsewhere.

> **Important:** if the key is removed from the Credential Manager (for
> example by resetting your Windows profile), the encrypted data cannot
> be read any more. Mail that is still on your mail server is downloaded
> again; local drafts and mail fetched with POP3 and deleted from the
> server are lost. Keep backups.

If Nail cannot find the key at start, it says that your data is locked
instead of showing empty folders.

### Turning it on or off

Turning encryption at rest on (or off) converts your existing data in
three steps — database, stored messages, search index — and shows the
progress. You can keep working meanwhile; searching may be unavailable
for a moment during the last step. If Nail closes or the computer loses
power, the conversion continues where it stopped on the next start. If it
stops with an error, the settings show the error and **Try again**.

Turning it off converts everything back and removes the key from the
Credential Manager.

### What it does not do

- It does not protect mail while Nail is running and you are signed in.
- Converting rewrites files, but it cannot erase what an SSD kept of the
  old, unencrypted files. Turn it on early, before much mail arrives, and
  use BitLocker for full-disk protection.
- It does not affect how mail is stored on your mail server.

## Searching encrypted mail

Mail that was sent to you end-to-end encrypted (OpenPGP or S/MIME) is
decrypted only for display. By default the search index gets only what
was visible outside the encryption: sender, recipients and the outer
subject (often just `...`). If you turn on **searching encrypted mail**
for an account, Nail also indexes the decrypted subject, text and
attachment names of that account's encrypted mail. The search index then
contains sensitive text — turn on encryption at rest as well. Turning the
option off removes that text from the index again.

## The sender check

Mail servers check whether a message really comes from the domain in its
`From` address and record the result in an `Authentication-Results`
header:

- **DKIM** — the sending domain signed the message;
- **SPF** — the server that delivered it may send for that domain;
- **DMARC** — the domain's own policy, combining both with the `From`
  address. This is the result the badge shows when it is present.

Anyone can write such a header into a message, so Nail only believes the
one your **own receiving server** added — recognized by the name you
entered under **Settings → Accounts → Sender checks** — and ignores the
others, including copies placed further down in the message. Without a
name there, Nail shows no sender badge for that account. **Suggest from
recent mail** fills in the server named in most of your recent mail;
compare it with the source of a message you received (the first word
after `Authentication-Results:` in the topmost such line) before you
save it: if your provider adds no such header, the name could come from
the senders.
"Aligned" in the details means the checked domain belongs to the same
organization as the `From` address.

*Sender confirmed* means the domain vouches for the message. It does not
mean the content is harmless: a scammer's own domain can pass these checks
too. *Sender check failed* means the message probably does not come from
where it claims.

## Link warnings

Nail looks at every link in a message and warns when:

- **the text shows another address than the target** — for example the
  text says `https://www.bank.example/login` but the link goes to
  `login.evil.example`. Links whose text is not an address ("Click here")
  are not compared;
- **the domain imitates another one** — international domain names that
  mix alphabets (a Cyrillic `а` in `аpple.com`) or consist only of letters
  that look like Latin ones. The warning shows the real, encoded name
  (`xn--pple-43d.com`) and the name it imitates;
- **the link is unusual** — it goes to a bare IP address, contains a
  user name before the `@` (`https://bank.example@evil.example/`, which
  really goes to `evil.example`), or points into your network or file
  system (`file:`, `smb:`).

## Attachment warnings

Nail warns about attachments that can harm your computer when opened:

| Warning | Examples |
| --- | --- |
| Program or script | `.exe`, `.scr`, `.bat`, `.cmd`, `.ps1`, `.js`, `.vbs`, `.msi`, `.jar`, `.hta` |
| Office file with macros | `.docm`, `.xlsm`, `.pptm`, `.xlsb` |
| Disguised name | `invoice.pdf.exe`, a long run of spaces before the real extension, or hidden characters that reverse the name on screen |
| Disk image | `.iso`, `.img`, `.vhd`, `.vhdx` |
| Shortcut | `.lnk`, `.url`, `.scf` |
| Type does not match the name | a `.pdf` that declares itself a program |

Nail never opens attachments by itself. A warning means: only open the
file if you expected it and trust the sender.
