# Accounts

How to add mail accounts, change their settings and identities, and combine
several accounts into one with virtual accounts. Everything you set here is
saved in `accounts.kdl` in your configuration folder; the
[accounts reference](accounts-reference.md) describes that file if you
prefer to edit it by hand.

## Add an account

Open **Settings → Accounts** and choose **Add account** (or run **Add account** from the command palette, Ctrl+K).

1. **Address.** Enter your name and email address and click **Continue**.
   Nail looks up the server settings: from your provider's own
   configuration, from Mozilla's provider database, from DNS records of the
   domain, from Microsoft Autodiscover, or — as a last resort — by trying
   common server names such as `imap.<your domain>`.
2. **Servers.** Nail shows what it found, each proposal with its protocol.
   If there are several, pick one; IMAP comes first and is selected when the
   provider offers both IMAP and POP3. **Edit server settings** lets you
   change the protocol (IMAP or POP3), server name, port, security (TLS,
   STARTTLS or none) and user name.
   If nothing was found, you enter them yourself.

   Some answers cannot be verified: they arrived without encryption (anyone
   on your network, for example on public Wi-Fi, could have changed them),
   or they come from DNS records and point to servers outside your
   address's domain. Nail marks such proposals with a warning, never
   selects them on its own when a verified one exists, and shows their
   servers for you to check. Only continue with them if you know these are
   your provider's servers; otherwise your password goes to someone else.
3. **Sign in.** Choose how you sign in:
   - **Password:** type it. It is stored in the Windows credential manager,
     never in a file.
   - **Sign in with Google / Microsoft:** click the button; your browser
     opens the provider's sign-in page. Nail waits until you are done and
     then shows "Signed in." Nail never sees your Google or Microsoft
     password.
4. **Test.** **Test connection** connects to both servers and signs in.
   Each server shows "Connected and signed in." or what went wrong (wrong
   password, server not reachable, secure connection failed). Go **Back**
   to fix the settings, or **Add anyway** if you know the server is only
   unavailable right now.
5. **Done.** **Go to the inbox** opens the new account. Nail downloads your
   mail in the background.

### Signing in with Google or Microsoft

Google and Microsoft only let registered applications sign in. Until Nail
ships with its own registration, add the client id of an app you registered
to `accounts.kdl` (see [OAuth2 providers](accounts-reference.md)):

```kdl
oauth-provider "google" client-id="1234567890-abc.apps.googleusercontent.com"
```

Without it, **Sign in with Google** reports a configuration error. After
signing in, Nail keeps only the long-lived *refresh token* in the Windows
credential manager and asks the provider for short-lived access tokens when
it connects. If the provider withdraws the permission, the account shows
"Sign-in required"; sign in again in the account settings.

### Where passwords are kept

Passwords and sign-in tokens are stored in the Windows credential manager
under the name `nail`, never in `accounts.kdl` or any other file. The file
only holds a reference such as `keychain:nail/work/password`. Removing an
account deletes its entries.

## POP3 accounts

POP3 servers only offer an inbox, so a POP3 account gets local folders in
Nail: Inbox, Sent, Archive and Trash. Moving mail between them never
touches the server. Nail checks for new mail every 10 minutes (or as set
in the account settings) and when you click **Check for new mail** or press `F5`.

What happens on the server after a message was downloaded is up to you
(**Settings → Accounts → POP3 download**):

| Setting | Effect |
| --- | --- |
| **Keep messages on the server** off | Each message is deleted from the server right after Nail downloaded it |
| **Keep messages on the server** on (default) | Messages stay on the server… |
| … **Delete from the server after (days)** | …until they are this many days old (counted from the download) |
| … **Delete from the server when deleted in Nail** | …until you delete them for good in Nail (emptying Trash counts; moving to Trash does not) |

Nail never deletes a message from the server that it has not downloaded.

## Connection states

The dot next to each account shows its state, and the status bar names
accounts that need attention:

| State | Meaning |
| --- | --- |
| Online | Connected; new mail arrives by itself |
| Connecting… | Nail is (re)connecting |
| Offline | The server cannot be reached; Nail keeps trying. Everything you do is saved and sent when it is back |
| Sign-in required | The server rejected the password or the sign-in expired. Enter a new password in the settings |
| Disabled | You turned synchronization off for this account |

When an account or a folder fails, **Show details** in the status bar (and
under **Recent problems** in the account's settings) shows what Nail was
doing and the server's exact answer; the account's
[connection log](connection-log.md) shows the whole exchange, without mail
content or passwords.

## Account settings

**Settings → Accounts** lists your accounts. Select one to change:

- its name and address, and whether it is synchronized;
- the incoming and outgoing servers; for IMAP how often folders without
  push updates are checked, for POP3 whether mail stays on the server and
  when it is deleted there;
- the password (leave **New password** empty to keep the current one);
- its **identities**.

**Remove account** deletes the account and its local copy of the mail from
Nail. Nothing changes on the server.

### Identities

An identity is an address you send from: the account's main address, an
alias, a plus-address. Each identity has a sender name, an optional
reply-to address and a signature. The first identity is the account's
default. When you reply, Nail sends from the identity the message was
addressed to.

## The unified inbox: "All accounts"

"All accounts" at the top of the account list shows all your accounts as
one. Its Inbox holds the mail of every Inbox, its Sent folder every Sent
folder, and so on. Folders that exist in only one account appear under that
account's name. Every message still belongs to its own account: flagging,
moving or deleting it happens there.

## Virtual accounts

A virtual account is your own combination of accounts — for example "Work"
made of two work addresses. **Settings → Accounts → New virtual
account**:

- **Name** and **Accounts in it.**
- **Merge Inbox, Sent, Drafts, Archive, Junk and Trash of all accounts**
  (on by default) combines the folders with the same purpose.
- **Add merged folder** combines other folders by hand, for example
  "Receipts" made of `Receipts` in one account and `Bills` in another.
  Folders you do not merge appear under a subfolder per account.
- **Identity for new messages** — the sender of new messages written in
  this virtual account. Without one, the first account's default is used.
- **Rules** choose the sender by recipient: `*@client.com` → your work
  identity. `*@*.client.com` also matches subdomains; a full address
  matches only that address. The first matching rule wins until you pick a
  sender yourself.

The same message received by two accounts (a mailing list, for example)
appears once in a virtual account. Moving a message into a merged folder
puts it into the matching folder of its own account (for example, **Archive**
of a virtual account means the Archive of the account the message is in).

Deleting a virtual account changes nothing on any server.
