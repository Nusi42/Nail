# Junk mail, snooze and notifications

This page explains how Nail decides what is junk, how snoozed messages come
back, and when you get a new-mail notification.

## Junk mail

Nail uses two sources to decide whether a message is junk:

1. **Your server.** If **Trust the server's Junk flags** is on (the default),
   a message the server delivered into its Junk folder or marked as junk
   counts as junk. A message marked as not junk counts as not junk, even when
   it is in the Junk folder. These decisions also teach Nail's own filter.
2. **Nail's own filter** (optional, off by default). It learns from you.
   Every time you choose **Mark as junk** or **Not junk**, it learns
   which words, senders and links show up in your junk and in your normal
   mail. Each account has its own filter.

Nail's filter gives each arriving message a junk score from 0 to 100:

| Score | What happens |
| --- | --- |
| At or above your threshold (default 90) | The message is marked as junk and, if **Move junk to the Junk folder** is on, moved there. |
| 20 or less | The message is treated as normal mail. |
| In between | Nail is **unsure**. The message stays where it is. You can see the score in the reader. |

The filter starts working only after you have marked enough messages as junk
**and** as not junk (10 of each by default). Until then it does nothing.

**Nail never deletes mail.** Junk is only marked and moved, so you can always
get a message back. **Not junk** moves a message from the Junk folder
back to the Inbox. You can undo a mark, and the filter forgets what it
learned from that mark.

Nail does not store the text of your mail in its filter. It keeps only
scrambled fingerprints of the words, and counts for each one. The filter
does not grow without end: it remembers the words of your latest 1,000
marks (so you can change your mind about them), and when it holds very many
words it forgets the ones seen in only one message.

## Snooze

Snoozing hides a message until a time you pick. The message moves to a
hidden folder called **Snoozed** on your server, and it gets a `$Snoozed`
mark so other mail programs can recognize it. At the time you picked, Nail
moves it back to the folder it came from. If that folder no longer exists,
the message goes to the Inbox.

- If you already have a folder called **Snoozed** (made by you or another
  mail program), Nail leaves it alone and keeps showing it; its own hidden
  folder is then called **Nail Snoozed**.
- Mail that ends up in Nail's hidden folder without a wake-up time (for
  example moved there by another program) comes back to the Inbox the next
  time Nail starts, so nothing stays hidden for good.

- Nail keeps the wake-up times on your computer. Snoozed mail comes back on
  time even without a connection. The move on the server happens when you
  are online again.
- If Nail was closed when a message was due, it comes back as soon as Nail
  starts.
- Times follow your computer's clock and time zone. "Tomorrow at 8:00" means
  8:00 on your clock, also on the night the clocks change. If you pick a
  time that the clock skips (for example 2:30 on the night the clocks go
  forward), the message comes back at 3:30.
- You can snooze a message for at most ten years.

## New-mail notifications

Nail checks the following, in this order, to decide whether a new message
shows a notification:

1. **Notifications off:** no notification.
2. **Quiet hours:** no notification. Messages that arrive during quiet hours
   do not notify later either.
3. **Rules:** a rule with the action "Notify" or "Don't notify" decides.
4. **Junk:** messages filed as junk do not notify, unless a rule says so.
5. **Folder setting:** a notification setting for the folder decides.
6. **Account setting:** a notification setting for the account decides.
7. **Default:** new mail in the Inbox notifies. If you choose "All folders",
   every folder notifies except Sent, Drafts, Trash and Junk.

**Quiet hours** use your computer's time zone. A range like 22:00–07:00
crosses midnight. It belongs to the day it starts on: if you choose Friday
only, Saturday 03:00 is quiet and Friday 03:00 is not. The end time is not
included, so at 07:00 notifications start again. On the nights the clocks
change, quiet hours still end at 07:00 on your clock.

**Bursts:** when several messages arrive in the same folder within a few
seconds, you get one notification ("5 new messages in Work") instead of
five. It shows the newest message's sender and subject if previews are on.

**Where they appear:** the desktop app shows Windows notifications. In a
browser (remote access), Nail shows the browser's notifications once you
allow them (**Allow notifications in this browser** in Settings →
Notifications).

When snoozed messages come back, you also get a notification (one per
folder they return to), unless notifications are off or it is quiet time.
