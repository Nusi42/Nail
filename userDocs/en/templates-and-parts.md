# Row templates, parts and keyboard

This page covers what the layout and theme references only touch on: the
template language used for message-list rows, the complete list of part names
and states the UI renders (so you can style them in `theme.kdl`), and how the
panes behave with the mouse and keyboard.

## Row templates

A `message-list` has one `row` per line. Each row is text with fields in
braces:

```kdl
message-list page-size=100 {
    row "{from.name | bold-if-unread}  {date | relative}"
    row "{subject | truncate(80)}"
    row "{preview}"
}
```

### Fields

| Field | Shows |
| --- | --- |
| `from` / `from.name` | The sender's name, or the address when there is no name |
| `from.email` | The sender's address |
| `to` | All recipients, comma-separated |
| `subject` | The subject |
| `date` | The date (use a date filter, see below) |
| `preview` | The first words of the message |
| `unread`, `flagged`, `has_attachments` | `true` / `false` |
| `account` | The account the message belongs to |
| `tags` | The message's tags (use `chips`) |
| `count` | Messages in the conversation (`threads="collapsed"`), or copies of the message in a virtual account |
| `participants` | The people of the conversation, oldest first (use `names(n)`); for a single message its sender |
| `latest_from` | The sender of the conversation's newest message (on a single message: its own sender; use `names(1)`) |

An unknown field shows nothing.

### Filters

Filters follow the field, separated by `|`. You can chain several.

| Filter | Effect |
| --- | --- |
| `relative` | "5 minutes ago", "yesterday", "3 weeks ago", in your language |
| `short-date` | The time for today, day and month for this year, otherwise a short date |
| `truncate(n)` | At most `n` characters, then `…` |
| `bold-if-unread` | Shows the field in bold while the message is unread |
| `chips` | Shows a list as separate chips (`message-list.row.chip`) |
| `names(n)` | At most `n` names of a list of people: with more, the first `n - 1` and the newest, with `…` between them. You are written "me"; people who sent unread mail are bold (`message-list.row.participant`, states `me` and `unread`) |
| `if-many` | Shows a number only when it is more than 1 (for `count`: the conversation's count pill, empty for a single message) |
| `then("text")` | Writes `text` after a value that is not empty, inside the field's element (`message-list.row.suffix`): `{latest_from \| names(1) \| then(":")}{preview}` gives "Grace Hopper: All set for…", and no colon when there is no sender |

A misspelled filter, a missing argument (`truncate` needs a whole number) or
unbalanced braces make Nail skip that row and log a warning; if no row is
valid, the default rows are used. Write `{{` and `}}` for literal braces.

Templates are plain data: they cannot run code or load anything.

### How rows look in the theme

Each field becomes an element named after the field:
`{from.name}` → `message-list.row.from`, `{date | relative}` →
`message-list.row.date`, `{has_attachments}` →
`message-list.row.has-attachments`. Text between fields is
`message-list.row.text`; each row line is `message-list.row.line`. A line
carries `data-fields` with the fields it shows: the Compact density hides
every line that shows `preview`, so conversation rows have two lines there.

```kdl
part "message-list.row.date" { color "$color.fg-muted" }
part "message-list.row:unread .subject" { font-weight 700 }
```

## Parts and states

Everything in the window carries a part name you can style. Besides the parts
listed in the theme reference, the UI renders these:

| Part | Where |
| --- | --- |
| `app.status` | "Loading…" or a connection error before the layout appears |
| `layout.unknown-widget`, `layout.missing` | A widget kind or window the layout names but Nail does not know |
| `account-switcher.error`, `folder-tree.error`, `message-list.error`, `message-view.error` | A request failed |
| `folder-tree.list` | The list inside the folder tree |
| `message-list.loading`, `message-view.loading` | Waiting for data |
| `message-list.spacer` | Invisible space above and below the visible rows |
| `message-list.row.text`, `message-list.row.chip`, `message-list.row.count`, `message-list.row.participants`, `message-list.row.participant`, `message-list.row.separator`, `message-list.row.latest-from`, `message-list.row.suffix` | See "Row templates" |
| `message-list.row.expander` (+ `.icon`) | The chevron of a conversation row with `threads="expandable"` (states `expanded`, `single`: a conversation of one message keeps the space but shows no chevron) |
| `message-list.child` (+ `.rail`, `.from`, `.preview`, `.date`) | A message under an opened conversation row (`threads="expandable"`) |
| `thread-view.card` (+ `.header`, `.toggle`, `.avatar`, `.recipients`, `.actions`), `thread-view.fold`, `thread-view.feed` | The reader's card feed (`thread-view mode="cards"`); `thread-view.item` is an alias of `thread-view.card` for one release |
| `message-view.header.label`, `message-view.header.value` | Label ("From") and value of a header line |
| `message-view.text` | The body of plain-text mail |
| `message-view.attachment.name`, `message-view.attachment.size` | Inside an attachment |

States (`part "x:state"`):

| Part | States |
| --- | --- |
| `message-list.row` | `selected`, `unread`, `flagged`, `answered`, `has-attachments`, `thread`, `expanded` |
| `message-list.child` | `selected`, `unread`, `flagged`, `from-me` |
| `thread-view.card` | `expanded`, `unread`, `selected`, `from-me` |
| `folder-tree.item`, `account-switcher.item`, `tabs.tab` | `selected` |
| `toolbar.button` | `disabled` |
| `status-bar.item` (sync) | `idle`, `syncing`, `error`, `offline` |

Calendar, contact and task widgets (M3a) render parts starting with
`calendar.`, `mini-month.`, `agenda.`, `calendar-list.`, `task-list.`,
`contact-list.`, `contact-card.` and `pim.` (editors, dialogs, reminder
toasts); their events carry the states `selected`, `today`, `outside`,
`busy`, `recurring`, `overridden`, `read-only`, `tentative`, `free`,
`all-day` and `dragging`, tasks `done` and `overdue`. Positions and
calendar colours come from the theme's geometry rules; see the default
theme (`defaults/theme.kdl`) for every part. Since M5b the calendar's
navigation bar is `calendar.toolbar` (Today and Previous / Next in
`calendar.nav`, the range `calendar.title`, the view switch
`segmented[field="calendar-view"]`, New event), an event's texts are
`event-card.title`, `event-card.time` and `event-card.location`, and a
calendar's colour square is its check box `calendar-list.toggle`.

The M5b shell (ADR 0109) adds: `nav-rail` (`.logo`, `.item` with the state
`active`, `.label`, `.badge`, `.spacer`, `.separator`, `.account`),
`view-host` (`.view` per view), the folder pane's `folder-tree.compose`,
`folder-tree.section` (`.header`, `.dot`, `.title`, `.unread-count`),
`folder-tree.account`, `folder-tree.favorite`, `folder-tree.smart-folder`,
`list-header` (`.title`, `.counts`, `.chips`), the toolbar's `toolbar.more`,
and the settings page `settings-page` (`.nav`, `.nav.item`, `.search`,
`.card`, `.title`, `.breadcrumb`).

Conditions (`part "x[key=\"value\"]"`): `folder-tree.item[role="inbox"]`,
`folder-tree.item[depth="1"]` (nesting level), `message-list.row[account="…"]`,
`toolbar.button[action="mail.reply"]`, `status-bar.item[item="folder-counts"]`,
`message-view.header.field[field="date"]`, `message-view.attachment[mime="application/pdf"]`.

## HTML mail

HTML mail is shown in an isolated frame: scripts never run, and nothing is
loaded from the internet. When a message contained remote images or fonts, a
banner (`message-view.remote-content-banner`) says so. The reader's font,
size, width and quote colours come from the theme's `message` section; in dark
mode, `html-dark-mode` decides whether HTML mail is left alone, inverted, or
recoloured.

## Panes and keyboard

- **Resize panes** by dragging a divider, or focus it with Tab and press the
  arrow keys (hold Shift for bigger steps). Sizes respect the `min-sizes` /
  `max-sizes` of the split.
- **Tabs** (narrow windows) switch by click or with the arrow keys, Home and End.
- **Folders:** arrow keys, Home and End move through the folder tree.
- **Key bindings** come from the `keys` section of `layout.kdl`. A binding with
  `context="list"` works while the message list has focus, `context="reader"`
  while the message view has focus, and `context="global"` everywhere in
  the main window. While you type in a text field only chords with Ctrl, Alt
  or Meta and Escape work (for example `Ctrl+Enter` to send). Sequences such
  as `"g r"` wait one second for the next key.
- **Windows** (composer, settings, …) keep the keyboard to themselves:
  Escape closes them, and main-window shortcuts do not fire inside them.

The default keys are listed in [Getting started](getting-started.md#keyboard-shortcuts).

## Language and colour mode

Nail follows your system language. English is complete; texts missing in
another language appear in English. In the browser you can add `?lang=de` to
the address to pick a language, and `?mode=dark` (or `light`,
`high-contrast`, `system`) to override the theme's colour mode.
