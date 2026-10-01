# Calendar, contacts and tasks

This page explains Nail's calendar views, the event editor (including
repeating events), contacts and groups, merging duplicate contacts, tasks,
reminders, and where calendars and address books come from.

Calendars, contacts and tasks are **widgets** like the message list, so your
`layout.kdl` decides where they appear (see [Layout options](#layout-options)
below). Every change you make is saved on this computer at once and sent to
the server in the background, also when you are offline for a while.

## Opening the calendar, contacts and tasks

In the default layout the main toolbar has **Calendar**, **Contacts** and
**Tasks** (keys **g c**, **g p** and **g t**). The calendar window shows the
mini-month, your calendars and tasks beside the calendar views, with a
toolbar for **Sync calendars and contacts**, **New task**, **New contact**,
**Import calendar or contacts** and **Calendars and address books**. In the
calendar window the keys **t** (today), **n** / **p** (next / previous),
**d**, **w**, **m**, **y**, **a** (day, week, month, year, agenda) and **c**
(new event) work.

Nail starts with a calendar (**My calendar**), a task list (**My tasks**)
and an address book (**My contacts**) on this computer, plus **Collected
addresses** for addresses you wrote to.

## Calendar views

A calendar widget shows one of these views. The buttons at the top switch
between them, and **Previous**, **Today** and **Next** move through time.

| View | What it shows |
| --- | --- |
| Day, Days, Week, Work week | A time grid: the hours of the day from top to bottom, one column per day, all-day events in a row on top |
| Month | Every day of the month with its events; **n more** opens the day when not all fit |
| Year | Twelve small months; days with events are marked. Choose a day to see it |
| Agenda | The events of the next days as a list |
| Free time | When you are free during the working day (at least 30 minutes by default). Choose a free stretch to plan an event there |

Events are shown in their calendar's colour. Overlapping events stand side by
side. An event that repeats says so when you move to it with a screen reader.

The **mini-month** (a small calendar for any window) and **today's agenda**
follow the same day: choosing a day in the mini-month shows it in every
calendar view. Weeks start on the day your language uses (Monday in German,
Sunday in American English).

### Moving events

- **With the mouse:** drag an event to another time or day; drag its bottom
  edge to make it longer or shorter. In the month view and the all-day row,
  drag an event to another day.
- **With the keyboard:** move to the event and press **Alt + ↑ / ↓** (15
  minutes earlier / later), **Alt + ← / →** (a day earlier / later; in the
  month view **Alt + ↑ / ↓** move by a week), **Alt + Shift + ↑ / ↓**
  (shorter / longer). **Delete** deletes the event.

Moving or deleting one occurrence of a repeating event asks whether the
change is for **this occurrence**, **this and following occurrences** or
**all occurrences**.

Events of read-only calendars (subscriptions, shared calendars you cannot
change) cannot be moved.

### Keyboard in the calendar

| Where | Keys |
| --- | --- |
| Hours of the time grid | Arrow keys move between hours and days, **Home** / **End** to the first / last hour, **Enter** creates an event at that hour |
| Month view | Arrow keys move between days, **Home** / **End** to the start / end of the week, **Enter** shows the day |
| Mini-month, year view, date picker | As in the month view; **Page Up** / **Page Down** go to the previous / next month, with **Shift** a year |
| An event | **Enter** opens it, **Delete** deletes it, **Alt** + arrows move it |

Your layout can also bind keys to the calendar actions (for example **t** for
today); see [Layout options](#layout-options).

## The event editor

Open an event by clicking it (or **Enter**), or create one with **New
event**, by double-clicking an hour or a day, or with **Enter** on an hour.

- **Title, calendar, location, description, web page.**
- **When:** start and end, or **All day**. Dates and times are typed or
  changed part by part with the arrow keys; **Choose…** opens a month to pick
  the day. The order of day, month and year and the 12- or 24-hour clock
  follow Nail's language, not the system's.
- **Time zone:** events can be in any time zone, or have **no time zone**
  (the same clock time everywhere, for example "lunch at noon" while
  travelling). The end can be in another zone than the start (a flight).
  **Also show the times in** shows the start and end in a second zone while
  you edit.
- **Repeat:** daily, weekly on chosen days, monthly (on the same day, or on
  the first / second / … / last weekday of the month), yearly; every n days /
  weeks / months / years; ending never, after a number of times or on a
  date. A sentence below says what you chose ("Every 2 weeks on Monday,
  Thursday, until 31 Dec 2026"). Repetitions made in other programs that
  this editor cannot show are kept as they are ("custom rule") unless you
  replace them.
- **Video calls and dial-in:** links to join the call.
- **Attendees:** add people by address; suggestions come from your
  contacts. Each attendee has a role (chair, required, optional, for
  information) and shows their answer. (Sending invitations arrives later.)
- **Reminders:** any number, a time before or after the start or end.
- **Status** (confirmed, tentative, cancelled), **Show me as** (busy, free)
  and **Visibility**.

When you open one occurrence of a repeating event, choose what to change:

| Choice | What happens |
| --- | --- |
| This occurrence | Only this occurrence changes. The repetition itself cannot be changed here |
| This and following occurrences | The series ends before this occurrence and a new series with your changes starts with it |
| All occurrences | The whole series changes. Moving this occurrence moves every occurrence by the same amount |

The delete button follows the same choice.

## Reminders

When a reminder is due, Nail shows it in a small box at the bottom of the
window (the desktop app also shows a system notification). Choose **Open**
to see the event or task, **Snooze** to be reminded again (in 5 to 60
minutes, or tomorrow morning), or **Dismiss**. Dismissing is saved in the
event or task (on the server too), so your other devices stop reminding you;
snoozing applies to this computer.

- Reminders follow your [notification settings](junk-snooze-notifications.md):
  with notifications switched off you see the box in Nail but no system
  notification. During **quiet hours** reminders still appear, without a
  sound.
- Reminders that came due while Nail was not running appear when it starts.
  Those more than two hours late are listed but do not pop up a notification.
- All-day events and events without a time zone remind at the clock time of
  the computer's time zone; when you travel, they follow.
- Turn a calendar's reminders off in its settings in the calendar list.
  Subscriptions do not remind you unless you ask for it
  (`alarms=#true`, see the [accounts reference](accounts-reference.md)).

## Contacts

The contact list searches by name, address, phone number or organization,
and filters by address book and group. Choose a contact to see its card:
photo, every email address, phone number, address, web page and extra field,
birthday and anniversary, notes, groups, the contacts it is linked with, and
upcoming meetings with that person.

- **New contact** / **Edit** open the contact editor: name parts and how the
  name is shown, nicknames, organization and job title, any number of email
  addresses, phone numbers, addresses, web pages and chat addresses (each
  with a kind such as home or work, and one marked as preferred), birthday
  and anniversary (the year may be unknown), notes, categories, extra fields
  and a photo. Choose another address book to move the contact.
- **Select several contacts** with **Ctrl**-click or **Shift**-click, or with
  **Shift + ↑ / ↓** in the list; **Space** adds or removes the contact you
  are on. **Delete** deletes the selection, **Enter** edits.
- **Groups…** manages contact groups: create, rename, delete, choose
  members (contacts of the same address book) and extra addresses.
- The **Collected addresses** book holds addresses Nail collected from
  mail you sent. It stays separate, so your real address books stay clean.
- When you write an email, the **To**, **Cc** and **Bcc** fields suggest
  addresses from your contacts (address books with suggestions on), the
  Collected addresses and the people you wrote to recently, best matches
  first. A contact's address beats a collected one; addresses you use often
  and recently rank higher.

### Merging duplicates

**Find duplicates…** lists contacts that look like the same person (same
email address, phone number or name). With two or more contacts selected,
the button merges those instead. Choose the main contact and, where the
contacts differ, which name, organization, birthday and so on to keep;
email addresses, phone numbers and addresses are combined (untick the ones
you do not want).

Contacts in the **same address book** as the main contact are merged into it.
Contacts in **other address books** stay where they are, **linked** to the
main contact, so each account keeps its own card. The card lists linked
contacts; **Unlink** separates one again.

## Tasks

The task list shows open tasks of your task lists, subtasks below their
task. Tick a task to complete it; a repeating task moves on to its next
date instead. Overdue tasks are marked.

- **Add a task to …** adds a task quickly; **New task** opens the editor.
- The task editor has the title, task list, **Subtask of**, status, percent
  done, priority, start and due date (each with or without a time), notes,
  location, web page, categories, repetition (needs a due date) and
  reminders. Its **Subtasks** section completes, opens and adds subtasks.
- A task created from an email links back to it (**Show the email**).

Keyboard: **↑ / ↓** move between tasks, **← / →** hide / show subtasks,
**Space** completes, **Enter** opens the editor, **Delete** deletes the task
and its subtasks.

## Calendars, address books and subscriptions

The **calendar list** shows your calendars and task lists by where they come
from. Untick a calendar to hide it from the views (it still syncs and
reminds). **Settings** renames it, changes its colour and turns its reminders
on or off. You can create calendars and task lists on this computer.

**Calendars and address books…** shows every source with its state (up to
date, syncing, offline, sign-in failed) and the last sync; **Sync now** and
**Remove** work per source. To add one, choose:

- **Subscription (ICS / webcal):** the address of a calendar feed, such as
  public holidays. Subscriptions are read-only.
- **CalDAV server** or **CardDAV server:** choose the mail account whose login
  to use (Nail finds the server from the account's domain), or enter the
  server address, user name and password.

**Test** checks the connection and lists what it found; **Add** saves the
source in `accounts.kdl` (see the [accounts reference](accounts-reference.md)).

**Import…** reads an iCalendar file (`.ics`) or a vCard file (`.vcf`), shows
what it holds, and imports it into the calendar or address book you choose.

### Opening files and links with Nail

When Nail is your app for `.ics` and `.vcf` files and `webcal:` links (see
[Installing Nail](installing.md)), opening one starts Nail (or uses the
running one):

- an `.ics` or `.vcf` file opens the import window with its preview —
  choose the calendar or address book and **Import**;
- a `webcal:` link opens **Calendars and address books** with the address
  filled in as a new subscription — **Test**, then **Add**.

Importing an item that is already in the chosen calendar or address book
(the same `UID`) updates it instead of adding it twice.

### Create an event or task from an email

**Create event from email** and **Create task from email** (reader toolbar
and the message's right-click menu) open the editor filled in from the
selected message: its subject as the title, the sender and recipients as
attendees (not you), and a link back to the message. When the message
carries an invitation or event (`.ics`), its details are used instead.

## CalDAV and CardDAV servers

**Finding the server.** With a mail account chosen, Nail looks for the
calendar and contact server of the account's domain; with an address
entered, it starts there. In order it tries: the address you entered (and
its `/.well-known/caldav` or `/.well-known/carddav`), the domain's DNS
service records (`_caldavs._tcp` / `_carddavs._tcp`, asked through your
system's DNS), the domain's `/.well-known/` address, and for Gmail
addresses Google's servers. **Test** shows what it found.

**Signing in.** Passwords are sent only over encrypted connections
(`https:`); plain `http:` is accepted only for a server on this computer.
Google calendars and contacts use Google sign-in (OAuth2) instead of a
password. Passwords and sign-ins stay in the Windows credential store.

**Syncing.** Nail checks each source every 15 minutes (subscriptions every
6 hours, or what the feed asks for; at least 5 minutes, see `refresh=`) and
right after you change something. While the server cannot be reached the
source shows **Offline** and Nail tries again, waiting longer each time; your
changes wait on this computer. **Sign-in failed** means the server refused
the password or sign-in: correct it in the source's settings.

**When the same item changed on the server and here**, the server's version
wins and is shown; your version is kept. The contact card
says **Changed here and on the server** — choose **Keep the server's
version** or **Keep my version** (which then replaces the server's).

Messages you may see:

| Message | Meaning |
| --- | --- |
| The server could not be reached / did not answer in time | No connection; Nail tries again |
| The secure connection to the server failed | The server's certificate is not trusted (or the address is wrong) |
| The server refused the sign-in | Wrong user name or password, or the sign-in expired |
| No calendar or contact server was found | Discovery found nothing: enter the server's address |
| The item changed on the server | Another device changed it first; see conflicts above |
| The server's sync information expired | Nail lists the collection again from the start (nothing is lost) |

## Calendar and contact files

Nail keeps every calendar item and contact **as it came** (the original
iCalendar or vCard text) and changes only what you edit, so properties
other programs added (and Nail does not show) survive your edits and the
next sync. Some details:

- vCard 2.1 files (older Outlook and phones) are read; a 2.1 card that you
  edit is saved as vCard 3.0. vCard 3.0 and 4.0 stay in their version.
- Time zones are recognized by their names (IANA names such as
  `Europe/Berlin`, Windows / Outlook names such as "W. Europe Standard
  Time", and the zone definitions in the file). A zone Nail does not
  recognize gives a warning in the import preview; its times are converted
  using the definition in the file.
- Subscriptions that give times without a zone are read in the feed's own
  time zone (`X-WR-TIMEZONE`), as Google and Outlook.com publish them.
- Characters in Windows (ANSI) and UTF-16 files are converted.

## Layout options

The widgets and their options (all optional):

| Widget | Options |
| --- | --- |
| `calendar` | `view` (`day`, `multi-day`, `week`, `work-week`, `month`, `year`, `agenda`, `free-time`), `views` (the views the switcher offers, separated by spaces), `days` (days of `multi-day`, `agenda`, `free-time`), `calendars` (calendar ids; default all shown), `time-zone`, `secondary-zone` (a second hour column), `hour-start`, `hour-end`, `scroll-to` (hour shown first), `max-events` (month), `duration` (free-time minutes), `work-start`, `work-end` (free-time hours), `header=#false` (no navigation bar) |
| `mini-month` | `time-zone`, `calendars` |
| `agenda` | `days` (default 1: today), `time-zone`, `calendars` |
| `task-list` | `lists` (task list ids), `show-completed`, `sort` (`due`, `priority`, `title`, `manual`), `quick-add` |
| `contact-list` | `address-books` (ids), `page-size` |
| `contact-card` | — |
| `calendar-list` | — |

Actions for toolbars, menus and keys: `calendar.goto-today`,
`calendar.next`, `calendar.previous`, `calendar.view "month"`,
`calendar.new-event`, `calendar.calendars`, `calendar.event-from-message`,
`tasks.new`, `tasks.from-message`, `contacts.new`, `contacts.groups`,
`contacts.merge`, `pim.import`, `pim.sources`, `pim.sync`, and the window
openers `pim.calendar.open`, `pim.contacts.open`, `pim.tasks.open`. The
default layout's windows are `calendar`, `contacts` and `tasks`, with the
toolbar `calendar`. Key bindings can use the contexts `calendar`,
`contacts` and `tasks`:

```kdl
keys context="calendar" {
    bind "t" "calendar.goto-today"
    bind "n" "calendar.next"
    bind "p" "calendar.previous"
    bind "m" "calendar.view \"month\""
}
```

Every part of these widgets can be styled in `theme.kdl` (parts such as
`calendar.event`, `calendar.month.day`, `contact-list.item`,
`task-list.item`, `pim.datetime`, `pim.toast`; states such as `:today`,
`:selected`, `:outside`, `:busy`, `:recurring`, `:done`, `:overdue`). The
calendar colour reaches an event as its left border colour.
