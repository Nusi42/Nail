# Accounts reference (`accounts.kdl`)

Your mail accounts, sending identities and virtual accounts are stored in
one file, `accounts.kdl`, in your Nail configuration folder (next to
`theme.kdl` and `layout.kdl`). Nail writes this file when you add or change
an account in the app, and you can also edit it by hand. When Nail changes
the file it keeps your comments, blank lines and formatting, and only
touches the part that changed.

**Passwords are never stored in this file.** Passwords, sign-in tokens and
certificates live in your operating system's keychain (on Windows: the
Credential Manager). The file only says *where* in the keychain a secret
is, for example `secret="keychain:nail/work/password"`. Nail refuses a file
that contains a `password=` or similar property.

If the file has a mistake, Nail tells you the line and column of the
problem.

## The short version

An account needs a name, an address and an incoming server. Everything else
has sensible defaults:

```kdl
account "work" email="ada@work.example" {
    imap host="imap.work.example"
    smtp host="smtp.work.example"
}
```

This uses TLS on the standard ports (IMAP 993, SMTP 465), logs in with the
address `ada@work.example` and a password from the keychain entry
`keychain:nail/work/password`, and sends as `ada@work.example`.

## File structure

```kdl
undo-send-delay 10

oauth-provider "<id>" client-id="…" { … }

account "<name>" email="…" { … }

virtual-account "<name>" { … }

calendar-subscription "<name>" url="…"

caldav "<name>" account="…"
carddav "<name>" account="…"
```

All parts are optional and may appear in any order. Accounts and virtual
accounts are shown in the order they appear in the file.

## Accounts

```kdl
// Work mail
account "work" name="Work" email="ada@work.example" {
    imap host="imap.work.example" port=993 tls="implicit" {
        auth "password" username="ada" secret="keychain:nail/work/password"
    }
    smtp host="smtp.work.example" port=587 tls="starttls" {
        auth "password" username="ada" secret="keychain:nail/work/password"
    }
    identity "ada@work.example" name="Ada Lovelace"
}
```

The name after `account` (`"work"`) identifies the account. It may use
lowercase letters, digits, `-`, `_` and `.`, must start with a letter or a
digit, is at most 64 characters long and must be unique among accounts and
virtual accounts. The name `all` is reserved for the built-in "All accounts"
view.

| Property | Meaning | Default |
| --- | --- | --- |
| `email` | The account's main address (required) | — |
| `name` | Name shown in Nail | the account name |
| `enabled` | `#false` stops syncing the account without removing it | `#true` |
| `save-sent-copy` | Put a copy of sent mail into the Sent folder | automatic (skipped for servers that do it themselves, such as Gmail) |

Inside the account:

| Node | Meaning |
| --- | --- |
| `imap`, `pop3`, `gmail` or `exchange` | The incoming server (exactly one) |
| `smtp` | The outgoing server (optional; without it the account only receives; not with `gmail` or `exchange`) |
| `identity` | A sending identity (any number; the first is the default) |
| `sync` | Which folders to synchronize and how much to download |
| `authserv-id` | Which receiving servers' sender checks to trust (see below) |
| `sieve` | Server-side rules and vacation replies over ManageSieve (IMAP accounts; see below) |

### Trusted sender checks (`authserv-id`)

Your provider's receiving server records whether a message passed the
DKIM, SPF and DMARC checks in an `Authentication-Results` header, starting
with the server's name (its *authserv-id*). Anyone can put such a header
into a message, so Nail shows the **sender badge** only for headers from
your own receiving server:

```kdl
account "work" email="ada@work.example" {
    imap host="imap.work.example"
    authserv-id "mx.work.example" "mx2.work.example"
}
```

Without `authserv-id` Nail shows no sender badge for the account: it
never trusts a server name on its own, because a sender can write any
`Authentication-Results` header when your provider adds none. In
**Settings → Accounts → Sender checks**, **Suggest from recent mail**
proposes the name found on top of most of your recent inbox mail (only a
name that belongs to the server that received the mail); it counts once
you save it. Check it first: in the source of a received message it is
the first word after `Authentication-Results:` in the topmost such
header.

### Server-side rules (`sieve`)

Many IMAP providers run filters on the server with **Sieve**, managed over
**ManageSieve**. With a `sieve` node, Nail uploads the rules it can express
in Sieve (and your vacation reply) to the server, so they apply even while
Nail is closed; the rules editor shows for each rule whether it runs on the
server or in Nail. Nail signs in with the account's IMAP login.

```kdl
account "work" email="ada@work.example" {
    imap host="imap.work.example"
    sieve
}
```

| Property | Meaning | Default |
| --- | --- | --- |
| `host` | ManageSieve server | the IMAP host |
| `port` | Port number | 4190 |
| `tls` | `"starttls"`, `"implicit"` or `"plaintext"` (test servers only) | `"starttls"` |
| `script` | Name of Nail's script on the server (Nail makes it the active one; other scripts stay untouched) | `"nail"` |
| `enabled` | `#false` keeps the settings but runs every rule in Nail | `#true` |

Without a `sieve` node every rule runs in Nail. POP3 accounts cannot use
`sieve`.

Which rules can run on the server, the vacation reply and what Nail
changes there: [Server-side rules and vacation replies](server-side-rules.md).

## Servers

| Property | Meaning | Default |
| --- | --- | --- |
| `host` | Server name or IP address (required) | — |
| `tls` | `"implicit"` (TLS from the start), `"starttls"` (upgrade after connecting), or `"plaintext"` (no encryption — only for test servers on your own computer) | `"implicit"` |
| `port` | Port number | IMAP 993 / 143, POP3 995 / 110, SMTP 465 / 587 (implicit / otherwise) |

IMAP servers also accept:

| Property | Meaning | Default |
| --- | --- | --- |
| `poll-interval` | Seconds between checks of folders that are not watched live (at least 30) | 300 |
| `compress` | Use compression when the server offers it | `#true` |
| `max-connections` | Most connections Nail opens at once (1–16) | 4 |

POP3 servers download mail into local folders and accept:

| Property | Meaning | Default |
| --- | --- | --- |
| `poll-interval` | Seconds between downloads (at least 30) | 600 |
| `leave-on-server` | Keep messages on the server after downloading | `#true` |
| `delete-after-days` | With `leave-on-server`: delete from the server this many days after downloading | never |
| `delete-when-deleted-locally` | With `leave-on-server`: delete from the server when you delete the message in Nail | `#false` |

SMTP servers also accept `dsn=#true` to ask for delivery failure reports.

### Gmail (`gmail`)

A `gmail` node connects to Gmail through the Gmail API instead of IMAP:
labels, threads and categories come over as Gmail has them, and Nail sends
through the same connection, so a Gmail account has no `smtp` node. It
signs in with your Google account (`auth "oauth2" provider="google"`; see
[OAuth2 providers](#oauth2-providers)). Every property is optional:

| Property | Meaning | Default |
| --- | --- | --- |
| `poll-interval` | Shortest time in seconds between checks for changes (at least 10) | 30 |
| `max-poll-interval` | Longest time in seconds between checks when nothing changes | 300 |
| `host`, `port`, `tls` | Only for test servers on your own computer | `gmail.googleapis.com`, 443, `"implicit"` |

Gmail cannot push changes to a desktop program, so Nail checks for them:
often right after something changed, less often while the mailbox is
quiet. Your labels become tags in Nail (renaming a label renames the tag;
a new tag on a Gmail message creates the label), and the inbox, sent mail,
drafts, spam, trash and "all mail" become folders. Gmail files the mail
you send in Sent itself, so Nail adds no copy; chats are not synced; label
colours must be from Gmail's palette (the label editor offers only those).
**Add account** proposes this kind of account for `gmail.com`,
`googlemail.com` and Google Workspace domains.

```kdl
account "personal" email="ada@gmail.example" {
    gmail poll-interval=60 {
        auth "oauth2" provider="google"
    }
}
```

A Gmail address set up with `imap` and `smtp` keeps working as an IMAP
account.

### Microsoft Exchange (`exchange`)

An `exchange` node connects to Microsoft Exchange. Mail, calendars,
contacts and tasks all come over this one connection, and Nail sends
through it too, so an Exchange account has no `smtp` node. `engine`
chooses how Nail talks to the server:

- `engine="graph"` (the default) — **Exchange Online / Microsoft 365**
  through Microsoft Graph. Sign in with your Microsoft work or school
  account: `auth "oauth2" provider="microsoft"`. Microsoft To Do lists
  appear as task lists.
- `engine="ews"` — **Exchange Server in your organization** (2016, 2019,
  Subscription Edition) through Exchange Web Services. Give the server's
  `host` (or the `url` that account setup found). Sign in with
  `auth "windows"` (your Windows sign-in, see below), `auth "password"`
  (sent only over an encrypted connection) or `auth "oauth2"` for hybrid
  setups.

Microsoft is retiring Exchange Web Services in Exchange Online. An account
that uses `engine="ews"` with Exchange Online shows a warning: switch it to
`engine="graph"`.

| Property | Meaning | Default |
| --- | --- | --- |
| `engine` | `"graph"` or `"ews"` | `"graph"` |
| `host` | The Exchange server (EWS) | `graph.microsoft.com` for Graph; for EWS the host of `url` |
| `url` | EWS only: the address of the EWS service | `https://<host>/EWS/Exchange.asmx` |
| `poll-interval` | Shortest time in seconds between checks for changes (at least 10) | 30 |
| `max-poll-interval` | Longest time in seconds between checks when nothing changes | 300 |
| `streaming` | EWS only: let the server announce changes at once | `#true` |
| `server-rules` | Run Nail rules as Exchange inbox rules where Exchange can express them | `#true` |
| `port`, `tls` | Only for test servers on your own computer | 443, `"implicit"` |

Exchange categories become tags in Nail, with the category colours.
Focused Inbox shows as **Focused** and **Other**, and the search
`focus:other` finds mail sorted into Other. Out-of-office replies and
server-side rules use Exchange's own (instead of Sieve). Shared mailboxes
and mailboxes you have delegate access to appear as extra folders under
the account; meeting rooms can be found and booked from the event editor.
**Add account** asks Exchange's Autodiscover service which engine to use
and proposes IMAP + SMTP as a fallback when the server offers them.

```kdl
account "work" email="ada@contoso.example" {
    exchange {
        auth "oauth2" provider="microsoft"
    }
}

account "office" email="ada@corp.example" {
    exchange engine="ews" host="mail.corp.example" {
        auth "windows"
    }
}
```

```kdl
account "home" email="ada@home.example" {
    pop3 host="pop.home.example" leave-on-server=#true delete-after-days=14
    smtp host="smtp.home.example" tls="starttls"
}
```

Exchange accounts are best set up through **Add account**: Autodiscover
fills in the node. Nail asks before it sends a sign-in to a server outside
your address's domain, and always for servers found through the unencrypted
HTTP redirect method or a DNS SRV record; the global Microsoft Autodiscover
service receives the address you set up (only for domains whose mail
Microsoft hosts). See
[Microsoft Exchange](exchange.md#what-autodiscover-sends-where).

Exchange Server (EWS) specifics: there is no Focused Inbox; Outlook
client-only rules block placing Nail's rules as inbox rules until they are
removed; answering an invitation without sending the answer only updates
the calendar; rooms carry no capacity or building. Moving an Exchange
Online account from `engine="ews"` to `engine="graph"` needs a new
Microsoft sign-in (tokens are per service), and its shared mailboxes are
then added by address.

### Signing in (`auth`)

Each server may have an `auth` line. Without one, Nail signs in with a
password, using the account's `email` as the user name.

| Method | Example | Notes |
| --- | --- | --- |
| Password | `auth "password" username="ada" secret="keychain:nail/work/password"` | `mechanism=` may force `"plain"`, `"login"`, `"scram-sha-256"` or, for POP3, `"apop"` |
| OAuth2 | `auth "oauth2" provider="google"` | For Gmail and Microsoft 365 / Outlook.com. `provider` is `"google"`, `"microsoft"` or an `oauth-provider` from this file. `mechanism=` may force `"xoauth2"` or `"oauthbearer"` |
| Client certificate | `auth "client-certificate" secret="keychain:nail/work/client-certificate"` | The keychain entry holds the certificate and its key |
| Windows sign-in | `auth "windows"` | Exchange servers in your organization (`engine="ews"`) only. Without `username` Nail signs in as you are signed in to Windows (Kerberos, or NTLM where Kerberos is not available) and stores no password. With `username="CORP\ada"` it uses that account and its password from the keychain. `package=` may force `"kerberos"` or `"ntlm"` (default `"negotiate"`) |
| None | `auth "none"` | For an outgoing relay that needs no sign-in |

`username` defaults to the account's `email`. `secret` defaults to
`keychain:nail/<account>/password` for passwords,
`keychain:nail/<account>/oauth2` for OAuth2 and
`keychain:nail/<account>/client-certificate` for certificates. Incoming and
outgoing servers of the same account therefore share one password unless
you give them different `secret`s.

A `secret` always has the form `keychain:<service>/<name>`. Nail's own
entries use the service `nail`.

```kdl
account "gmail" name="Gmail" email="ada@gmail.com" {
    imap host="imap.gmail.com" {
        auth "oauth2" provider="google"
    }
    smtp host="smtp.gmail.com" {
        auth "oauth2" provider="google"
    }
}
```

## Identities

An identity is an address you send from. Every account has at least one; if
you list none, Nail uses the account's `email`.

```kdl
account "work" email="ada@work.example" {
    imap host="imap.work.example"
    smtp host="smtp.work.example"
    identity "ada@work.example" name="Ada Lovelace" reply-to="team@work.example" composer="markdown" posting="top" {
        bcc "archive@work.example"
        signature "Ada Lovelace\nAnalytical Engines Ltd." markdown="**Ada Lovelace**  \nAnalytical Engines Ltd."
        mail-style css="body { font-family: Georgia, serif; }" code-theme="base16-ocean.light"
    }
    identity "support@work.example" name="Support" composer="plain"
}
```

| Property / node | Meaning |
| --- | --- |
| the argument | The sender address |
| `name` | Your name as recipients see it |
| `reply-to` | Where replies should go, if not to the sender address |
| `id` | A unique name for the identity, needed only if two identities use the same address (default: the address) |
| `composer` | How new messages start: `"plain"` (default), `"html"` or `"markdown"` (see [Writing mail](composer.md)) |
| `posting` | Where replies put the quoted original: `"bottom"` (default: your text below the quote) or `"top"` |
| `bcc "…" …` | Addresses that always get a blind copy |
| `signature "…"` | Your signature as plain text; `\n` starts a new line. `html="…"` and `markdown="…"` give the HTML and Markdown variants; a missing one is converted from another |
| `mail-style` | How HTML and Markdown mail looks: `css="…"` is a stylesheet Nail builds into every message (default: a plain, readable one), `code-theme="…"` the colours of code blocks in Markdown (see [Markdown mail](markdown-mail.md#how-it-looks)) |
| `crypto` | Signing and encryption defaults for this identity (see below) |
| `send-as` | Send for another mailbox, typically a shared or delegate Exchange mailbox whose address is the identity's: `"send-as"` (the mail is from the mailbox alone; you need the "Send As" permission) or `"on-behalf"` (recipients see "you on behalf of the mailbox"; you need "Send on Behalf") |
| `sender` | With `send-as="on-behalf"`: your own address shown as the sender (default: the account's address) |

Identity names (`id`, or the address) must be unique across all accounts.

### Signing and encryption (`crypto`)

> **Not active yet:** Nail reads and saves these settings, but signing and
> encrypting mail arrive in a later version. Until then, a message that
> should be signed or encrypted (by these defaults or the composer's
> toggles) is **not sent**; Nail shows an error instead of sending it
> unprotected.

```kdl
account "work" email="ada@work.example" {
    imap host="imap.work.example"
    identity "ada@work.example" name="Ada Lovelace" {
        crypto technology="openpgp" sign=#true encrypt=#true openpgp-key="0123 4567 89AB CDEF 0123 4567 89AB CDEF 0123 4567" prefer-encrypt="mutual"
    }
}
```

| Property | Meaning | Default |
| --- | --- | --- |
| `technology` | `"openpgp"`, `"smime"` or `"auto"` (whatever every recipient supports; OpenPGP when both work) | `"auto"` |
| `sign` | Sign new messages | `#false` |
| `encrypt` | Encrypt new messages when every recipient has a key | `#false` |
| `openpgp-key` | Your OpenPGP key: its fingerprint (40 or 64 hexadecimal digits; spaces allowed) | your newest usable key for the address |
| `smime-cert` | Your S/MIME certificate: its SHA-256 fingerprint (64 hexadecimal digits) | your newest usable certificate for the address |
| `autocrypt` | Send Autocrypt headers so others can encrypt to you, and learn keys from theirs | `#true` |
| `prefer-encrypt` | Autocrypt preference: `"mutual"` (encrypt automatically when your correspondent says so too) or `"nopreference"` | `"nopreference"` |
| `protect-headers` | Hide the subject of encrypted mail from servers (it shows as `...` there) | `#true` |

## What to synchronize (`sync`)

By default Nail downloads complete messages of every folder and watches the
inbox live for new mail. `sync` changes that.

With `bodies="headers-only"` (or `"recent"` for older mail) Nail keeps
only each message's header lines: the list shows sender, subject and date
as usual, and the rest is downloaded when you open a message (or choose
**Download full message**). Searching words of such bodies asks the server
([Searching](search.md#mail-without-a-downloaded-body)). With `"recent"`,
bodies of mail older than `days` days are removed again from this device;
their search entries stay. POP3 accounts always keep complete messages.

```kdl
account "work" email="ada@work.example" {
    imap host="imap.work.example"
    sync bodies="recent" days=90 {
        folder role="inbox" bodies="full"
        folder path="Lists/rust" idle=#true
        folder role="junk" enabled=#false
    }
}
```

| Property | Meaning |
| --- | --- |
| `bodies` | `"full"` (whole messages), `"headers-only"` (bodies are downloaded when you open a message) or `"recent"` (whole messages from the last `days` days, headers for older ones) |
| `days` | Number of days for `bodies="recent"` |

Each `folder` rule picks folders by `path` (the full folder name on the
server, e.g. `"Lists/rust"`) or by `role` (`"inbox"`, `"sent"`, `"drafts"`,
`"archive"`, `"junk"`, `"trash"`) and changes one or more of:

| Property | Meaning | Default |
| --- | --- | --- |
| `bodies`, `days` | As above, for these folders | the `sync` setting |
| `enabled` | `#false` skips the folder entirely | `#true` |
| `idle` | Watch the folder live for new mail | only the inbox |

When several rules match a folder, later rules win.

## Virtual accounts

A virtual account shows several accounts as one: one inbox, one Sent folder,
one search. Your mail stays in its real account; removing a virtual account
changes nothing on any server. Nail always offers a built-in "All accounts"
view as well.

```kdl
account "work" email="ada@work.example" {
    imap host="imap.work.example"
}
account "home" email="ada@home.example" {
    imap host="imap.home.example"
}

virtual-account "everything" name="Everything" default-identity="ada@home.example" {
    members "work" "home"
    map "Receipts" {
        folder account="work" path="Receipts"
        folder account="home" path="Bills"
    }
    identity-rule "*@client.example" identity="ada@work.example"
}
```

| Property / node | Meaning | Default |
| --- | --- | --- |
| `name` | Name shown in Nail | the virtual account's name |
| `members "…" …` | The accounts to combine (required; real accounts only) | — |
| `merge-roles` | Combine folders with the same role (Inbox, Sent, Drafts, Archive, Junk, Trash) | `#true` |
| `map "<folder>" { folder account=… path=… }` | Combine other folders by hand under one name | — |
| `default-identity` | Identity for new messages (an identity of a member) | the first member's default identity |
| `identity-rule "<pattern>" identity=…` | Send from this identity when a recipient matches: `"*@client.example"` (a domain), `"*@*.client.example"` (its subdomains) or a full address | — |

Folders that are neither merged by role nor mapped appear under each
account's own name inside the virtual account, so nothing is hidden.

When you reply, Nail sends from the identity the original message was
addressed to.

## OAuth2 providers

Gmail and Microsoft accounts sign in with OAuth2. Nail knows the providers
`google` and `microsoft`; each needs the client id of an app registration:

```kdl
oauth-provider "google" client-id="1234567890-abc.apps.googleusercontent.com" client-secret="keychain:nail/oauth/google"
oauth-provider "microsoft" client-id="00000000-0000-0000-0000-000000000000"
```

Other providers need their endpoints too:

```kdl
oauth-provider "corp" client-id="nail" auth-url="https://login.corp.example/authorize" token-url="https://login.corp.example/token" {
    scopes "https://mail.corp.example/imap" "offline_access"
}
```

| Property / node | Meaning |
| --- | --- |
| `client-id` | The registered client id (required) |
| `client-secret` | Keychain reference of a client secret, if the provider issued one |
| `auth-url`, `token-url` | The provider's endpoints (`https://` only); required for providers other than `google` and `microsoft` |
| `scopes "…" …` | Permissions to request, if not the provider's mail scopes |

## Calendars and contacts

Nail always has **local** calendars, task lists and address books on your
computer (including the "Collected" address book of people you wrote to);
they need nothing in this file. Calendar subscriptions and calendar or
contacts servers are added here, next to your mail accounts.

### Calendar subscriptions (`calendar-subscription`)

A subscription shows a published calendar (holidays, a team schedule) as
a read-only calendar and refreshes it regularly:

```kdl
calendar-subscription "holidays" name="Holidays" url="webcal://example.org/holidays.ics" color="#d33"
```

| Property | Meaning | Default |
| --- | --- | --- |
| `url` | The calendar's address: `https://`, `webcal://` or `webcals://` (required; fetched over `https:`, never from this computer or the local network) | — |
| `name` | Name shown in Nail | the name the calendar gives itself |
| `color` | `#RGB`, `#RRGGBB`, `#RRGGBBAA` or a colour name | the theme's |
| `alarms` | `#true` shows the reminders the published calendar contains | `#false` |
| `refresh` | Minutes between refreshes (5 to 10080) | what the calendar suggests, else 360 |
| `enabled` | `#false` stops refreshing without removing the calendar | `#true` |

A protected calendar takes an `auth` node like a server (below); its
password goes only to the address in `url`, not to another server the feed
forwards to. Changes to a subscription take effect at its next refresh: a
new `url` or `alarms` fetches the whole calendar again, and a new `name`,
`color` or `alarms` updates the calendar in Nail.

### Calendar and contacts servers (`caldav`, `carddav`)

`caldav` connects your calendars and task lists on a CalDAV server,
`carddav` your address books on a CardDAV server. Most providers offer
both next to mail; then name the mail account, and Nail signs in with the
same login and finds the servers from your address:

```kdl
account "work" email="ada@work.example" {
    imap host="imap.work.example"
}
caldav "work-calendars" account="work"
carddav "work-contacts" account="work"
```

A server without a mail account needs its address and a login:

```kdl
caldav "home-calendars" url="https://dav.home.example/" {
    auth "password" username="ada" secret="keychain:nail/pim/home-calendars/password"
}
```

| Property | Meaning | Default |
| --- | --- | --- |
| `account` | The mail account whose login the server uses | — |
| `url` | The server's address (`https://`; `http://` only for `localhost`) | found from the account's email address |
| `name` | Name shown in Nail | the server's name |
| `refresh` | Minutes between checks for changes (5 to 10080) | 15 |
| `enabled` | `#false` stops syncing without removing your data in Nail | `#true` |
| `auth` | How to sign in, as for mail servers ([Signing in](#signing-in-auth)) | the account's login; required without `account`, with a `username` |

Without `account`, the password is looked up in the keychain under
`keychain:nail/pim/<name>/password`. The names after `caldav`, `carddav`
and `calendar-subscription` follow the account-name rules, must be unique
among them, and `local` is reserved. Removing a mail account also removes
the servers that use its login (your data on the servers stays).

## Undo send

```kdl
undo-send-delay 10
```

Seconds a sent message waits in the outbox, so that you can still take it
back (0 to 600; default 10; 0 sends at once).

## Editor support

`nail config schema accounts` prints a JSON Schema of the file's structure
for editors that support schemas.
