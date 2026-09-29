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
| `imap` or `pop3` | The incoming server (exactly one) |
| `smtp` | The outgoing server (optional; without it the account only receives) |
| `identity` | A sending identity (any number; the first is the default) |
| `sync` | Which folders to synchronize and how much to download |
| `authserv-id` | Which receiving servers' sender checks to trust (see below) |

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

Without `authserv-id` Nail learns the id from your recent inbox mail (when
most of it carries the same topmost header) and remembers it. Set it when
the badge does not appear, or to be sure a wrongly learned id is never
used. Find the id in the source of a received message: it is the first
word after `Authentication-Results:` in the topmost such header.

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

```kdl
account "home" email="ada@home.example" {
    pop3 host="pop.home.example" leave-on-server=#true delete-after-days=14
    smtp host="smtp.home.example" tls="starttls"
}
```

### Signing in (`auth`)

Each server may have an `auth` line. Without one, Nail signs in with a
password, using the account's `email` as the user name.

| Method | Example | Notes |
| --- | --- | --- |
| Password | `auth "password" username="ada" secret="keychain:nail/work/password"` | `mechanism=` may force `"plain"`, `"login"`, `"scram-sha-256"` or, for POP3, `"apop"` |
| OAuth2 | `auth "oauth2" provider="google"` | For Gmail and Microsoft 365 / Outlook.com. `provider` is `"google"`, `"microsoft"` or an `oauth-provider` from this file. `mechanism=` may force `"xoauth2"` or `"oauthbearer"` |
| Client certificate | `auth "client-certificate" secret="keychain:nail/work/client-certificate"` | The keychain entry holds the certificate and its key |
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

> **Not active yet:** `bodies` and `days` are accepted and saved, but this
> version still downloads every message completely, whatever they say. Only
> `enabled` and `idle` take effect today. Saving space or bandwidth with
> `bodies="headers-only"` or `"recent"` comes in a later version.


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
| `bodies` | *Not active yet (see above).* `"full"` (whole messages), `"headers-only"` (bodies are downloaded when you open a message) or `"recent"` (whole messages from the last `days` days, headers for older ones) |
| `days` | *Not active yet.* Number of days for `bodies="recent"` |

Each `folder` rule picks folders by `path` (the full folder name on the
server, e.g. `"Lists/rust"`) or by `role` (`"inbox"`, `"sent"`, `"drafts"`,
`"archive"`, `"junk"`, `"trash"`) and changes one or more of:

| Property | Meaning | Default |
| --- | --- | --- |
| `bodies`, `days` | As above, for these folders (not active yet) | the `sync` setting |
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

## Undo send

```kdl
undo-send-delay 10
```

Seconds a sent message waits in the outbox, so that you can still take it
back (0 to 600; default 10; 0 sends at once).

## Editor support

`nail config schema accounts` prints a JSON Schema of the file's structure
for editors that support schemas.
