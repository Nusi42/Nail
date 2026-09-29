# Accounts

How to add mail accounts, change their settings and identities, and combine
several accounts into one with virtual accounts. Everything you set here is
saved in `accounts.kdl` in your configuration folder; the
[accounts reference](accounts-reference.md) describes that file if you
prefer to edit it by hand.

## Add an account

Click **Add account** in the toolbar (or in **Settings → Accounts**).

1. **Address.** Enter your name and email address and click **Continue**.
   Nail looks up the server settings: from your provider's own
   configuration, from Mozilla's provider database, from DNS records of the
   domain, from Microsoft Autodiscover, or — as a last resort — by trying
   common server names such as `imap.<your domain>`.
2. **Servers.** Nail shows what it found. If there are several proposals,
   pick one. **Edit server settings** lets you change the protocol (IMAP or
   POP3), server name, port, security (TLS, STARTTLS or none) and user name.
   If nothing was found, you enter them yourself.
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
made of two work addresses. **Settings → Virtual accounts → New virtual
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
appears once in a virtual account.

Deleting a virtual account changes nothing on any server.
