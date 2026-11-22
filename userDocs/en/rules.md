# Filter rules

Filter rules sort, tag and answer mail for you. A rule says **when** it
runs, **where** it applies, what a message must look like (**conditions**)
and what happens to a matching message (**actions**). Nail runs rules
itself, so they work whether the window is open or not.

Rules run in order, top to bottom. A rule sees what the rules above it
did: if one rule moves a message to "Reading", a later rule with the
condition `folder "Reading"` matches it.

## Managing rules

Your rules are in **Settings → Mail & junk** (the gear at the bottom of the
navigation rail, or **Ctrl+,**); **Filter rules** in the command palette
(**Ctrl+K**, or **Command palette** on the navigation rail) opens them in a
window of their own, and so does **Create rule from message** in a
message's **More** menu. Both look the same. The window takes most of the
screen; on a narrow screen (or a narrow window) the list of rules goes above
the editor.

The list on the left shows your rules in the order they run:

- **New rule** starts an empty rule. **Find a rule** filters the list by
  name.
- Each row shows the rule's name and when it runs ("when mail arrives",
  "by hand"). Click the check mark on the right to turn the rule on or off
  (with the keyboard: the **On** switch at the top of the editor). A rule
  that is off looks faded; it never runs by itself, but you can still run
  it by hand.
- Click a rule to open it in the editor. To select several rules, hold
  **Ctrl** and click each one, or click the first and **Shift**+click the
  last. With the keyboard: the **Up** and **Down** arrow keys move between
  rules, **Shift+Up** / **Shift+Down** add the next rule, **Ctrl+Up** /
  **Ctrl+Down** move without changing the selection, **Space** adds or
  removes the rule you are on (so rules that are not next to each other can
  be selected too), **Ctrl+A** selects every rule in the list and **Escape**
  goes back to just the rule you are on. While a rule with an Exchange
  badge has the keyboard focus, the badge shows each account's line below
  the rule; the first **Escape** hides it. The editor always
  shows the rule you clicked or moved to last; **Run** and **Test rule**
  then act on all selected rules (see [The footer](#the-footer)).
- To change the order, drag a rule by its handle (the dots on the left), or
  press **Alt+Up** / **Alt+Down** on it.
- With an Exchange account, a small **ⓘ** says that something about the
  rule is worth knowing there; its number counts all reasons it stays in
  Nail, over all your Exchange accounts. Point at it, or move the keyboard
  focus onto the rule, to read one line per account: where the rule runs
  for that account (on the server, or in Nail) and why.

With no rule open, the window offers **Create rule from message** (for the
message selected in the list) and a few templates to start from:
**Newsletters to a folder**, **Flag mail from someone** and **Archive old
read mail**. A template only fills in a new rule; nothing is saved until you
press **Save rule**.

## Editing a rule

At the top of the editor are the rule's name, an **On** switch and the
choice between **Editor** and **Text** (the arrow keys switch between them
too). Below, the editor reads like a sentence:

- **When** — when the rule runs: when mail arrives, when you send mail, by
  hand, on a schedule (every so many minutes) or when mail gets old (older
  than so many days).
- **Where** — the account it applies to (empty: every account).
- **If** — **Match all / any of:** the conditions. **+ Add condition** (a
  menu) adds a condition, or a group ("All of a group", "Any of a group",
  "None of a group") for conditions inside conditions.
- **Then** — the actions, in order. **+ Add action** adds one; **Up**,
  **Down** and **Remove** change them.

Every field that names a folder, an account, a tag, an address, a mailing
list or a header has a list to browse, search and pick from; you can always
type the value yourself instead.

### Values from this mail

When you open the rules window from a message (**Create rule from message**
or **Add to rule…** in the message's menu), or a message is selected in the
list, a small button right after a condition's value (before **Match upper
and lower case**; and after the folder of
**Move to folder** / **Copy to folder** and the tag of **Add tag** /
**Remove tag**) lists that message's values: the sender's address, name and
domain, the recipients, Reply-To, the subject or one of its words, the
mailing list, header names, the account, the folder and the tags. Picking
one fills in the field. A domain is matched as "ends with @domain", and an
address switches the condition to the field it came from (for example from
**From** to **Cc**).

**Add to rule…** in a message's menu opens a list of your rules: pick one
to open it in the rules window with that message's values at hand.

### The footer

The bar at the bottom of the window is always in view:

- On the left, what still keeps the rule from being saved shows as a small
  indicator ("2 things to fix", or "One error" when the text has a
  mistake). Point at it to read the details, or press it (Enter or Space) to
  open them; Escape closes them. The fields it means are marked. After
  saving, **Saved** shows for a moment; after a run, what the run did, with
  **Undo** for all of it.
- **Delete rule…** deletes the rule after asking; its recent runs go with
  it.
- **Run on** chooses the messages **Run** and **Test rule** act on:
  **Selected messages** (the messages selected in the list, with their
  number; only while some are selected), **Current folder** (the folder the
  list shows; the choice to start with when you opened the window from a
  message) or **Where the rule applies** (the rule's own folders, else the
  Inboxes).
- **Run** runs the rule on those messages now. A new rule, or one with
  changes you have not saved yet, shows **Save and run**: it saves the rule
  as you see it first, so the run never uses an older version. A rule that
  moves, archives or deletes mail asks first and says how many messages it
  matches.
- With several rules selected in the list, **Run** says how many ("Run 3
  rules") and runs them all, one after another in the order of the list,
  on the same **Run on** messages; **Undo** then takes back what all of them
  did. If the rule in the editor is one of them and has unsaved changes,
  it is saved first. If one of them moves, archives or deletes mail, Nail
  asks first and says how many matches they have together.
- **Test rule** and **Save rule**. **Save rule**, **Delete rule…** and the
  **Text** view always act on the rule in the editor only. **Save rule**
  keeps the window open; close it with × or `Esc`.

The questions (**Delete rule**, a run that moves or deletes mail) open in a
small window over the rules: the red button names the action, **Cancel** has
the focus and `Enter` never runs it.

Leaving a rule with unsaved changes — opening another rule, closing the
window, or pressing Escape — asks **Save changes to “…”?**: **Save** saves
the rule first, **Don't save** drops the changes, **Cancel** keeps editing.
While the rule cannot be saved yet, the question offers **Discard changes**
and **Keep editing**. In Settings → Mail & junk the rule is part of the
section's unsaved changes (see [Settings](settings.md#saving-your-changes)).

### Preview and recent runs

Under the editor are two tabs:

- **Preview** shows what **Test rule** found, without changing anything:
  how many messages match, and for each its subject (click it to open the
  message), sender, folder, what would happen ("Add tag: project", "Move to
  folder: Lists") and, folded, why it matched ("Subject matched: [rust-announce]
  …"). **Apply to these N** runs the rule on exactly these messages. Any
  change to the rule clears the preview, because it would describe an older
  version. After **Test rule** the keyboard focus is on the preview's
  summary. With several rules selected, the preview shows one part per
  rule, in the order of the list, each with its own **Apply to these N**.
  Each rule is tested on its own: when the rules really run, an earlier rule
  can move a message away before a later rule sees it, so the preview can
  show a message under a later rule that the run will not reach.
- **Recent runs (N)** lists the rule's runs: when each started, what
  started it (new mail, sent mail, a schedule, by hand), what it ran on, how
  many messages it matched and whether it is done. **Undo this run** takes
  back everything that run changed in one step (moves, tags, flags, read
  state, deletes to Trash). Where a run cannot be undone, point at its
  **Undo this run** (or move the focus onto it) to read why: it was undone
  already, it changed nothing, or it sent, copied or permanently deleted
  mail. If you changed some of the messages yourself in the meantime (moved
  one back, deleted one), undo restores the others and leaves those alone.

## Running rules by hand

A rule does not have to wait for new mail: you can run it on mail you
already have — for example right after writing it.

**In the rules window**, choose the messages in **Run on** and press
**Run** (see [The footer](#the-footer)).

**From the message list**: right-click a message (or press Shift+F10 or
the menu key) and choose **Run rule**. A list opens beside **Run rule** when you point at
it, or with the Right arrow key (Left closes it again). It shows **All
rules that are on** (when you have several), your rules that are on (each
name exactly as you wrote it), and
under **Off** the rules that are off — running a rule by hand works for
those too. The rule runs on the message, or on all selected messages when
you right-click one of them. **From the folder pane**: right-click a folder
and choose **Run rule** to run a rule on every message in that folder.

A small message then says what the run did, with **Undo** for all of it;
it disappears after a few seconds unless you point at it.

Running rules by hand is like **Run** on each of them: a rule that stops
processing ("Stop processing more rules") only ends its own run, the next
rule still runs.

## The editor and the text view

The **Text** view shows the same rule in the `rules.kdl` syntax described
below, with line numbers and colours. Edit it there and switch back to
**Editor**. Switching to **Text** and back without editing changes nothing;
a rule that is not finished yet (no name, an empty value) switches back
too, and what it still misses shows in the footer. Text with a mistake
stays in the text view: the footer's indicator says what is wrong, with
the line and column, and the line number is marked. **Discard text
changes** goes back to the rule as it was.

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

Accounts with ManageSieve can run rules **on the mail server** instead, so
they apply while Nail is closed; such a rule does not run again in Nail when
mail arrives. See [Server-side rules and vacation replies](server-side-rules.md).
