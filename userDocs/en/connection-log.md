# Connection log and error details

When an account does not synchronize, sign in or send, Nail shows what went
wrong — and, if you want to look closer, exactly what it said to the server
and what the server answered. None of this contains your mail or your
passwords.

## Show details

Every sync, sign-in and send error in the status bar has a **Show details**
button; so has every entry under **Settings → Accounts → (your account) →
Recent problems**. The details window says:

- **What Nail was doing** — for example *Opening the folder* or
  *Downloading messages*;
- the **protocol** (IMAP, SMTP, POP3, …) and the **folder**, when one was
  involved;
- the **command** that failed and the **server's answer**, word for word —
  for example `NO [SERVERBUG] Internal error occurred.` This is usually the
  most useful line for your provider's support;
- when it happened and the error class.

**Copy details** puts all of it on the clipboard; **Show in connection log**
opens the account's connection log at the moment of the error.

The last error of each account and of each folder is kept until a later
synchronization of it works, also when you restart Nail. Messages that
could not be downloaded are listed too ("2 messages in INBOX could not be
downloaded"); Nail tries them again with every synchronization, and the rest
of the folder synchronizes normally.

## The connection log

Open **Settings → Accounts → (your account) → Connection log**. It lists, as
they happen, the lines Nail exchanged with the account's servers: IMAP,
SMTP, POP3 and ManageSieve commands and answers, and the requests of
calendar and address book servers (CalDAV / CardDAV), Gmail, Microsoft Graph,
Exchange (EWS), sign-in token servers and server discovery.

- **Protocol** and **Show** filter the list (for example *Warnings and
  errors*).
- **Pause** stops following new lines; press it again to see them.
- **Copy** and **Export…** give you the whole log as text (in the desktop
  app Export asks where to save the `.txt` file; in a browser it downloads
  it). This is the text to send to support.
- **Clear** empties the log.

### What is recorded

- the commands Nail sends and the servers' status lines, with their response
  codes and texts;
- capabilities, folder names, message numbers (UIDs), flags, sizes and the
  server's receive dates;
- the encryption of each connection (TLS version and cipher) and a summary of
  the server's certificate (name, issuer, expiry, fingerprint);
- how long each connection or request took and how many bytes went each way;
- for web-based services, the request (method and address) and the answer's
  status; for errors also the error code and message the server returned.

### What is never recorded

- **Mail content**: message bodies and attachments appear only as their
  size, such as `{4211 bytes}`; subjects, names and other header values
  inside server answers become `<header>`;
- **search terms** (they become `<search term>`);
- **passwords, app passwords, tokens** and everything sent while signing in
  (`LOGIN`, `AUTHENTICATE`, `AUTH`, `PASS`, `Authorization` and cookie
  headers) — shown as `<redacted>`;
- **full mail addresses**: they are shortened, for example
  `n***@example.org`.

### Where it is kept

By default the connection log lives **in memory only**: the last 2,000
lines of each account, gone when Nail closes.

If you need to capture a problem across restarts, turn on **Keep connection
log on disk** under **Settings → Data & backup**. Nail then also writes the
same (already shortened) lines to files in its data folder
(`logs\connections\`), at most a few megabytes per account. Turning the
setting off deletes those files.

## The application log

Nail also writes a general application log, `logs\app.log` in its data
folder (in the desktop app and in `nail serve`), with the same protection:
no mail content, no passwords, shortened addresses. It is limited in size
and the oldest part is dropped. **Settings → Data & backup → Show recent
lines** shows its last lines; the [diagnostics bundle](diagnostics.md)
includes recent log lines too.

## Who can see it

Only you on this computer. People using the
[remote access](remote-access.md) see the connection log and the application
log only with administrator rights. AI agents connected through
[MCP](mcp.md) and plug-ins never see them.

## Sending a problem to support

1. When the error appears, choose **Show details**, then **Copy details**.
2. Choose **Show in connection log**, then **Export…** (or **Copy**) and send
   the text file along. Read it first if you like — it holds no mail content
   or passwords.
3. If the problem happens only now and then, turn on **Keep connection log on
   disk** until it happens again, then export the log and turn the setting
   off.
