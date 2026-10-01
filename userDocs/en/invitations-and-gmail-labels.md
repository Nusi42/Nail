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

If the sender of an invitation is not its organizer, the card shows a
warning first. Such invitations are never applied automatically; check
them before you answer.

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

Availability comes from your own calendars and, for other people, from
your calendar server. People Nail knows nothing about show "No
information".

The command `scheduling.find-time` (or the `freebusy-timeline` widget)
opens **Find a time** on its own: add people, choose the length, pick a
suggested time and **Create meeting** opens a new event with those people
invited.

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
(`tag:Travel`) and the tag commands work with them. The command
`gmail.apply-labels` lets you tick the labels of the selected message.

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
