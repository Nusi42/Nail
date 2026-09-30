# Server-side rules and vacation replies

If your mail provider supports **Sieve** filters (most IMAP providers that
offer "filters" or "out of office" in their webmail do, over a service
called **ManageSieve**), Nail can hand some of your filter rules and your
vacation reply to the server. The server then applies them the moment mail
arrives, even while Nail is closed and on every other device you use.

To switch it on, add a `sieve` node to the account (see
[Accounts reference](accounts-reference.md#server-side-rules-sieve)). Nail
signs in with the account's normal login.

## Where each rule runs

Every rule shows one of three places:

| Place | Meaning |
| --- | --- |
| **Server** | The rule is part of the script on the server. The server runs it on new mail; Nail does not run it again. You can still run it by hand. |
| **Nail** | The rule could run on the server, but right now Nail runs it. The rule shows why, for example: the rule is switched off, it also runs on a schedule or when sending, it only applies to folders other than the Inbox, the server lacks a feature the rule needs, or the new script is not uploaded yet. |
| **Nail only** | The rule uses something the server cannot do. The rule shows which condition or action is the reason. |

A rule never runs in both places, so nothing is moved or tagged twice.

The server only sees mail as it is **delivered to the Inbox**. A rule that
should also act on mail in other folders, or on mail you already have,
stays in Nail.

### What the server can do

Conditions:

- subject, any header, sender and recipient addresses, mailing list
  (`List-Id`), message text, size, date, sender checks (DKIM, SPF, DMARC)
- "equals", "contains", "starts with", "ends with", with or without case
  sensitivity, and regular expressions that use the common subset
  (letters, `.`, `*`, `+`, `?`, `{m,n}`, `[a-z]`, `\d`, `\s`, `\w`, `^`,
  `$`, groups and `|`)
- groups ("all of", "any of", "none of")

Actions: move, copy, archive, delete (to Trash or for good), tag and
untag, flag and unflag, mark read or unread, redirect, and stop.

### What stays in Nail

- search queries, attachment checks, the message's age, its flags and tags,
  folders other than the Inbox
- regular expressions with word boundaries (`\b`), case flags such as
  `(?i)`, lazy repetition (`*?`) or Unicode classes (`\p{…}`)
- forwarding as a new message, replying with a template, controlling
  notifications, moving or copying into another account
- tags with spaces or special characters, which the server cannot store

Some tests work slightly differently on the server: an address test also
looks at the whole header ("Ada Lovelace <ada@example.org>"), and letters
outside A–Z are compared without case only if the server supports it (if
not, the rule stays in Nail).

## Vacation replies

The vacation reply (out of office) is uploaded in the same script. It
answers each sender at most once in the number of days you choose, uses
your subject and text, and knows your own addresses (the account's
identities plus the ones you add), so it does not answer mail you were only
copied on through a list. With **Don't reply to mailing lists** on, it also
skips mailing lists, bulk mail and automatic messages.

A start and end date are checked by the server when it supports dates.
Otherwise Nail switches the reply on and off itself on those days, which
needs Nail to be running (or started) on the start and end day.

## Your own scripts on the server

Nail keeps exactly one script on the server, named `nail` unless you choose
another name, and makes it the active one. It never changes or deletes
your other scripts:

- If another script was active (for example one you made in your
  provider's webmail), Nail's script **runs it too** when the server
  supports that (the `include` feature); otherwise Nail remembers it and
  makes it active again when you switch server-side rules off.
- If a script with Nail's name already exists and Nail did not write it,
  Nail stops with a message instead of overwriting it. Pick another
  `script` name in the account's `sieve` node.

If the server rejects the script, the account shows the server's message
and every rule keeps running in Nail until the next successful upload.
