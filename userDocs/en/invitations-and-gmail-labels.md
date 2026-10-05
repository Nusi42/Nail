# Invitations, scheduling and Gmail labels

## Invitations in the reader

When a message contains a meeting invitation, Nail shows an invitation
card above the message text. The card tells you:

- what the message is: an invitation, an updated invitation, a
  cancellation, an answer to one of your meetings, or a new time someone
  proposes;
- whether your calendar already matches it ("Not answered yet", "Changes
  your calendar", "A newer version exists", …);
- when and where the meeting is, how it repeats, who organizes it, who is
  invited and what they answered;
- whether it overlaps something else in your calendar.

### Warnings about forged messages

Anyone can write any organizer or attendee into an invitation, so the
card checks who a message really comes from and shows a warning first
when something does not fit:

- **This invitation was not sent by the organizer** (or an answer does not
  come from the attendee it names).
- **Sent by someone acting for the organizer or participant**: the message
  itself claims that its sender may act for the organizer ("sent by").
  Nail cannot check that claim; assistants who send for their manager
  cause this warning too.
- **Names a different organizer than the event in your calendar**: a
  message about a meeting you already have, but from another organizer.
  Everyone invited to a meeting (and anyone who saw a forwarded
  invitation) knows the meeting's internal identifier, so such a message
  may try to take your meeting over.
- **Comes from someone who is not invited**: an answer to one of your
  meetings from a person who is not on the guest list (nor named as a
  stand-in by a guest).

Nail never applies such a message on its own — it is not added to your
calendar, and no update, cancellation or answer in it is applied — until
you choose to. Check with the organizer before you answer. Applying an
update by hand never changes who organizes the meeting. (Messages a
calendar service like Google Calendar sends for the organizer, with the
organizer as the sender, are recognised and show no warning.)

Published events (calendar data "for your information", without an
invitation) and messages about other people's meetings are never applied
on their own either, and neither is anything in a message marked as junk.

### Answering

Choose **Accept**, **Maybe** or **Decline**. You can add a note for the
organizer. Clear **Send my answer to the organizer** to only update your
own calendar without telling anyone. If you turned on **Ask before sending
an answer** in the invitation settings, Nail asks first.

For a repeating meeting that is already in your calendar you can answer
for **All occurrences** or for **One occurrence**: pick the day of the
occurrence with the date picker.

**Propose a new time** sends the organizer another start and end (and a
note). Your calendar does not change until the organizer agrees.

Depending on the message the card also offers **Add to calendar**,
**Update my calendar**, **Remove from calendar** (for cancellations),
**Ask for the latest version**, **Forward invitation** and **Open in
calendar**.

### The mail Nail sends

Answers, proposals, invitations, updates and cancellations leave like
any other mail: through the outbox of the address you were invited at (or
that organizes the meeting), so **Undo** works for a few seconds after you
answer. Each mail has a short readable summary (what, when, where, who,
your note) for people without a calendar program, plus the calendar data
itself — both inside the message, where Outlook, Google Calendar, Apple
Calendar and Thunderbird read it, and as an `invite.ics` attachment.

### Calendar servers that schedule themselves

Some CalDAV servers deliver invitations and answers themselves (RFC 6638
"auto-scheduling"). When the meeting is in a calendar on such a server,
Nail only saves the event on the server — the server then tells the other
people — and does **not** send its own mail, so nobody gets the same
message twice. The card says "Your calendar server sends your answer to
the organizer." Messages the server delivered to you are picked up with
each calendar sync.

Nail asks the server about this with the first sync and again once a
day. Until it knows (for example while the server cannot be reached),
Nail sends the mail itself and tells the server, in the event it saves,
to stay out of the delivery for that meeting — again, nobody gets two
messages.

### When you organize the meeting

Answers to your invitations show who answered and how; **Record the
answer** writes it into your event (Nail does this automatically unless
you turned it off). When someone proposes another time, the card shows
the proposal with **Accept the new time** (the meeting moves and everyone
gets the update) and **Decline the new time**.

In the event editor of a meeting you organize you can **Send
invitations** (to everyone or only to the people you choose, with a note)
and **Cancel meeting…**: cancel the whole meeting, only one occurrence of
a repeating meeting, or remove some attendees. The attendees are told.

### Invitation settings

The command `scheduling.settings` opens the invitation settings:

| Setting | Default |
| --- | --- |
| Put new invitations into my calendar (shown as unanswered) | on |
| Apply updates from organizers | on |
| Record answers to my invitations | on |
| Remove cancelled events | off — the card offers "Remove from calendar" |
| Ask before sending an answer | off |
| Calendar for new invitations | the default calendar |

## Finding a time

The attendee list of the event editor shows everyone's availability for
the meeting's day: busy times, times people might be busy, and
out-of-office times, with the meeting drawn on top. Nail names the people
who are busy at the chosen time and suggests times when everyone is free;
choose one to move the meeting there. Use **Back** / **Next** / **Today**
to look at other days.

Availability comes from your own calendars (subscriptions and other
read-only calendars do not count) and, for other people, from your CalDAV
calendar server (its scheduling outbox). People Nail knows nothing about
show "No information"; a server that does not share someone's
availability with you shows "availability could not be checked" for that
person.

The command `scheduling.find-time` (or the `freebusy-timeline` widget)
opens **Find a time** on its own: add people, choose the length, pick a
suggested time and **Create meeting** opens a new event with those people
invited.

## Gmail accounts

Nail reads Gmail through the Gmail API (not IMAP) when you add an address
of `gmail.com` or `googlemail.com`, or of a domain whose mail Google
hosts (Google Workspace): **Add account** proposes "Gmail through the
Gmail API" first and you sign in with Google. Details for `accounts.kdl`
are in the [accounts reference](accounts-reference.md#gmail-gmail).

- **New mail** is fetched by asking Gmail for changes regularly (there is
  no push for desktop programs): every `poll-interval` seconds while things
  change, less often while nothing happens (up to `max-poll-interval`), and
  at once when you press **Sync** or change something.
- **Folders** are Gmail's system labels: Inbox, Sent, Drafts, All Mail
  (shown as the archive), Spam, Trash. Archiving removes the Inbox label;
  a message is stored once, whatever labels it has.
- **Sent mail** is filed by Gmail itself; Nail does not add a copy.
- **Rules and invitations** act on new mail in the inbox only — not on
  your own sent mail, Spam or Trash (as for other accounts).
- **Settings → Accounts** shows a Gmail account as "Gmail API" and lets you
  change its shortest and longest check intervals.
- Chats are not synced.
- Gmail has no "answered" mark, so the replied mark exists only in Nail.
- Conversations are Gmail's own: messages Gmail puts in one conversation
  are one conversation in Nail, also without reply headers.
- Moving a message to another Gmail or an Exchange account copies it there
  (with its read and starred state) and removes it from this account;
  labels are not carried over.

## Gmail labels

For Gmail accounts the folder pane can show your labels (`label-list`
widget):

- Choose a label to see all its messages, in every folder.
- **New label** creates a label; **Edit** and **Delete** next to a
  label edit or delete it.
- Labels nest with `/` (`Work/Projects`), or choose **Nest under** in the
  label editor. Renaming a label renames the labels nested under it.
- The colours offered are the ones Gmail accepts.
- **In the label list** can hide a label or show it only while it has
  unread mail; **On messages** hides it on message rows.

In Nail, Gmail labels are tags with the same name: rules, search
(`tag:Travel`) and the tag commands work with them. A new tag on a Gmail
message creates the label in Gmail. The command `gmail.apply-labels`
(**Labels…** in the reader's toolbar, key `l`) lets you tick the labels of
the selected conversation, like Gmail: every message of it gets them, and
**Undo** takes them off again. Label changes made while offline are sent when the
account is back online.

## Gmail categories

The `category-tabs` widget shows Gmail's inbox categories — **Primary**,
**Social**, **Promotions**, **Updates**, **Forums** — with their unread
counts. Choose one to see only that part of the inbox; **All** shows the
whole inbox again. In the search bar the same filter is
`category:social`.

## Layout options

| Widget | Option | Values | Default |
| --- | --- | --- | --- |
| `invitation-card` | `attendees` | `collapsed`, `expanded` | `collapsed` |
| `invitation-card` | `conflicts` | `true`, `false` | `true` |
| `freebusy-timeline` | `hour-start`, `hour-end` | hours 0–24 | 8, 18 |
| `freebusy-timeline` | `duration` | minutes | 30 |
| `freebusy-timeline` | `time-zone` | an IANA zone | the system's |
| `label-list` | `show-hidden` | `true`, `false` | `false` |
| `label-list` | `show-counts` | `true`, `false` | `true` |
| `category-tabs` | `categories` | names separated by spaces | all five |
| `category-tabs` | `show-all` | `true`, `false` | `true` |
| `category-tabs` | `show-counts` | `true`, `false` | `true` |

Commands: `scheduling.accept`, `scheduling.tentative`,
`scheduling.decline` (answer the invitation of the selected message and
send the answer), `scheduling.find-time`, `scheduling.settings`,
`gmail.new-label`, `gmail.apply-labels`, `gmail.label-view "<label>"`,
`gmail.category "<category>"` (no argument: the whole inbox).
