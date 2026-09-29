# Filter rules

Filter rules sort, tag and answer mail for you. A rule says **when** it
runs, **where** it applies, what a message must look like (**conditions**)
and what happens to a matching message (**actions**). Nail runs rules
itself, so they work whether the window is open or not.

Rules run in order, top to bottom. A rule sees what the rules above it
did: if one rule moves a message to "Reading", a later rule with the
condition `folder "Reading"` matches it.

## Managing rules

Open **Filter rules** to see your rules in the order they run.

- **New rule** starts an empty rule. **Create rule from message** (on a
  selected message) starts one that already matches mail from the same
  sender or the same mailing list; you only add the actions.
- **Up** and **Down** change the order. The checkbox next to a rule
  turns it on or off; a rule that is off never runs by itself, but you can
  still run it by hand.
- **Test rule** checks the rule against the mail you already have, without
  changing anything. It lists the messages that match, what would happen
  to each, and why it matched ("Subject matched: [rust-announce] …").
- **Run now** runs a saved rule on your mail. **Recent runs** lists what
  each run did; **Undo this run** takes back everything the run changed in
  one step (moves, tags, flags, read state, deletes to Trash). Forwards,
  redirects, replies, copies and permanent deletes cannot be undone. If you
  changed some of the messages yourself in the meantime (moved one back,
  deleted one), undo restores the others and leaves those alone.

## The editor and the text view

The **Editor** view builds a rule from menus: add conditions, put them in
groups ("All of these", "Any of these", "None of these") and add actions.
The **Text** view shows the same rule in the `rules.kdl` syntax described
below; edit it there and switch back, or press **Check text** to see
mistakes with their line and column.

All rules are stored in `rules.kdl` in your configuration folder. You can
edit that file by hand too; Nail keeps your comments and layout when it
saves changes made in the app.

## rules.kdl

```kdl
// Newsletters go to Reading, without a notification.
rule "Newsletters" trigger="arrival" scope="virtual:everything" {
    when any {
        header "List-Id" exists=#true
        query "from:*@substack.com"
    }
    then {
        tag "newsletter"
        move "Reading"
        notify #false
    }
}
```

A rule is `rule "<name>"` with these optional properties:

| Property | Meaning |
| --- | --- |
| `id="…"` | A stable id (lowercase letters, digits, `-`, `_`, `.`). Without it the id comes from the name. |
| `enabled=#false` | Keep the rule without running it automatically. |
| `trigger="…"` | `arrival`, `on-send` or `manual` — shorthand for one `trigger` line. |
| `scope="…"` | Shorthand for one `scope` line. |

Inside the rule:

- `trigger "<kind>"` — when the rule runs; list several if needed:
  `arrival` (new mail), `on-send` (mail you send), `manual` (only by hand),
  `schedule every=<minutes>` (also `every="2h"`, `every="1d"`), and
  `older-than days=<n>` (checked every hour; the rule then only matches mail
  older than that).
- `scope "<where>"` — `"account:<name>"`, `"virtual:<name>"` (a virtual
  account), `"folder:<path>"` or `"role:<inbox|sent|drafts|archive|junk|trash>"`;
  folder and role scopes take `account="<name>"`. Without a scope a rule
  applies everywhere.
- `when` or `when any` — the conditions. `when` (or `when all`) needs all
  of them, `when any` one of them. Without `when` the rule matches every
  message.
- `then` — the actions, in order.

### Conditions

Text comparisons take exactly one of `equals=`, `contains=`,
`starts-with=`, `ends-with=` or `regex=` (a regular expression), and
optionally `case-sensitive=#true`. Upper and lower case are ignored by
default.

| Condition | Example |
| --- | --- |
| Header | `header "List-Id" exists=#true`, `header "X-Spam" exists=#false`, `header "X-Priority" equals="1"` |
| Addresses | `from contains="@example.com"`; also `to`, `cc`, `recipients` (To or Cc), `reply-to`, `sender` |
| Subject, text | `subject starts-with="[invoice]"`, `body contains="unsubscribe"` |
| Search query | `query "from:*@substack.com has:attachment"` (the search language, see [Search](search.md)) |
| Attachment | `attachment` (any), or `attachment { name ends-with=".pdf"; type starts-with="application/"; }` |
| Size | `size larger="5MB"`, `size smaller=10000` (bytes, or `k`, `m`, `g`) |
| Date | `date before="2026-01-01"`, `date after="2025-12-31"` (local days) |
| Age | `age older-than=30`, `age newer-than=2` (days) |
| Flags | `flag "seen" set=#false` (unread), `flag "flagged"`; also `answered`, `draft` |
| Tag | `tag "work"` |
| Account, folder | `account "work"`, `folder "Lists/Rust" account="work"`, `folder role="inbox"` |
| Sender checks | `dkim "pass"`, `spf "fail"`, `dmarc "pass"` (results: pass, fail, softfail, neutral, none, temperror, permerror) |
| Mailing list | `list-id` (from any list), `list-id contains="rust-lang"` |

Group conditions with `all { … }`, `any { … }` and `not { … }` (none of
the conditions inside may match); groups nest:

```kdl
rule "Important" trigger="arrival" {
    when {
        any {
            from contains="@boss.example"
            subject contains="urgent"
        }
        not {
            list-id
        }
    }
    then {
        flag
        stop
    }
}
```

In a quoted string a backslash starts an escape (`\"`, `\\`). For regular
expressions a raw string is easier: `subject regex=#"invoice #\d+"#`.

Sender checks read the `Authentication-Results` header your mail server
added (the topmost one only, because senders can forge lower ones).

### Actions

| Action | Meaning |
| --- | --- |
| `move "Reading"`, `move role="archive"` | Move to a folder (by path or special folder); `account="…"` for another account. |
| `copy "Backup"` | Copy to a folder. |
| `archive` | Move to the account's Archive. |
| `delete`, `delete permanent=#true` | Move to Trash, or delete for good. Nothing else happens to a deleted message. |
| `tag "newsletter"`, `untag "todo"` | Add or remove a tag. |
| `flag`, `flag #false` | Set or clear the flag. |
| `mark-read`, `mark-read #false` | Mark as read or unread. |
| `forward "a@example.com" identity="…"` | Forward as a new message. |
| `redirect "a@example.com"` | Pass the message on unchanged. |
| `reply "Thanks, I am away until Monday."` | Reply with a template (Markdown, above the quoted message; placeholders below). Nail never replies automatically to mailing lists, bulk mail, automatic mail or bounces. |
| `notify`, `notify #false` | Show or suppress the new-mail notification. This version shows no notifications yet; the choice is kept for when it does. |
| `stop` | Run no further actions or rules for this message. |

A `reply` template may contain placeholders that are filled in from the
message: `{sender.name}`, `{sender.email}`, `{subject}`, `{date}`,
`{identity.name}` and `{identity.email}` (for example
`reply "Hello {sender.name}, I am away until Monday."`). The values are
inserted as plain text: a subject that contains Markdown, links or HTML
shows up literally in your reply, so nobody can put a link or an image
into your automatic answer by choosing a clever subject.

### No mail loops

Mail a rule sends is marked as automatic (`Auto-Submitted`, RFC 3834) and
carries an `X-Nail-Loop` header. Nail uses the marks so that rules can
never send mail round in a circle:

- Rules never forward, redirect or answer a message that a Nail rule sent
  (your own or someone else's), a message one of your accounts already
  redirected, a bounce or an automatic reply such as an out-of-office
  notice.
- `on-send` rules do not run for mail that a rule sent. A rule "forward
  everything I send to my archive" forwards your messages, not its own
  forwards.
- Mailing-list mail and notifications can still be forwarded and
  redirected; they just never get an automatic reply.

```kdl
rule "Old newsletters" {
    trigger "older-than" days=60
    when {
        tag "newsletter"
    }
    then {
        archive
    }
}

rule "Away" trigger="arrival" enabled=#false {
    then {
        reply "I am away until Monday and will answer then."
    }
}
```

If `rules.kdl` has mistakes, Nail lists every one of them with its line
and column (see [the error list](customization.md#all-configuration-errors-at-once))
and keeps running the rules it had before.

## When rules run

- **New mail** (`arrival`): mail that arrives in the Inbox or another
  ordinary folder after the folder's first synchronization. Mail waiting
  for the rules is remembered, so it is still sorted after Nail was closed
  in between.
- **Sent mail** (`on-send`): the copy of a message you sent, in the Sent
  folder.
- **Schedules** (`schedule`, `older-than`): checked every minute while Nail
  runs; a scheduled rule looks at the Inboxes, or at the folders its
  `scope` names.
- **By hand** (`manual`, or **Run now**): on the Inboxes (or the folders
  in the rule's scope).

Every change a rule makes goes through the same queue as your own changes,
so it reaches the mail server when the account is online, and one run can
be undone as a whole. Forwards, redirects and replies are sent through the
outbox right away (without the undo-send delay), with a copy in Sent. When
you are offline they wait in the outbox and are sent once the account is
back online.
