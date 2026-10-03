# Writing mail

## The composer

**New message** (or `c`) opens the composer; **Reply**, **Reply all** and
**Forward** (`r`, `a`, `f`) open it filled in from the selected message.

- **From** chooses the identity you send as. New messages start with the
  default identity of the selected account (see [Accounts](accounts.md));
  replies use the identity the message was sent to. In a virtual account
  with identity rules, the identity follows the recipients you enter until
  you choose one yourself. Changing the identity swaps in its signature
  (in plain text and Markdown; in HTML, edit the signature by hand).
- **To**, **Cc**, **Bcc**: several addresses are separated by commas, as
  `ada@example.org` or `Ada Lovelace <ada@example.org>`. While you type,
  Nail suggests addresses you have written to or received mail from;
  `↑` / `↓` choose one, `Enter` or `Tab` takes it, `Esc` closes the list.
  **Add Cc** and **Add Bcc** show those fields.
- **Format** switches between **Plain text**, **HTML** and **Markdown**
  (see below). New messages start in the identity's format (`composer=` in
  the [accounts reference](accounts-reference.md#identities)).

`Esc` or the × button closes the composer without losing the message: **New
message** continues it. **Save draft** (`Ctrl+S`) keeps it as a draft;
**Discard** throws it away.

## Plain text, HTML and Markdown

- **Plain text** is sent as it is (as `format=flowed` text, so long lines
  wrap nicely on small screens).
- **HTML** is a word-processor style editor: the formatting bar has bold,
  italic, underline, strikethrough, headings, lists, quotes, code blocks,
  links, images and tables (with buttons to add or remove rows and
  columns). **Source** shows the HTML itself for editing by hand.
  Recipients get the formatted message and a plain-text version of it.
- **Markdown** is text with light markup (`**bold**`, `# Heading`,
  `- lists`, tables, code blocks). The preview next to the text shows
  exactly what will be sent; **Preview** turns it off and on. See
  [Markdown mail](markdown-mail.md).

Switching the format converts the message. Plain text to HTML or Markdown,
and Markdown to HTML, keep everything. Going from HTML or Markdown to plain
text, or from HTML to Markdown, can lose formatting (colours, fonts,
underlining, …); Nail then asks first: **Switch anyway** or **Keep
editing**.

What Nail sends is cleaned first: scripts, forms, frames and similar parts
of HTML are removed, so a message cannot run code on the recipient's side.

## Attachments and images

**Attach…** (or `Ctrl+Shift+A`) adds files; you can also drag files onto
the composer. Each attachment shows its name and size, with **Remove**
next to it. An image inserted with the formatting bar in HTML mode is sent
inside the message ("in text") instead of as a separate attachment. A
single file can be at most 25 MB.

If the text mentions an attachment ("attached", "attachment", …) but
nothing is attached, **Send** asks first: **Send anyway** sends it,
**Dismiss** stops asking for this message. Quoted text and the signature
are not checked.

## Replies, forwards and signatures

Replies quote the original under a line like "On 2026-09-29 10:00, Ada
Lovelace wrote:" — in plain text and Markdown as `>` lines, in HTML as an
indented quote block. Your reply goes below the quote, or above it for an
identity with `posting="top"`. **Reply all** includes every recipient
except your own addresses. Forwards include the original's sender, date,
subject and recipients, its text, and its attachments.

Your signature is added below a `-- ` line. An identity can have separate
HTML and Markdown signatures; otherwise Nail converts the one it has.

## Drafts

The composer saves the message a moment after you stop typing ("Draft
saved."). For IMAP accounts the draft is also stored in the account's
Drafts folder on the server, so you can continue it in another mail program;
each save replaces the earlier copy there, and sending or discarding the
message removes it.

Drafts survive closing Nail: open **Outbox**; under **Drafts** each saved
message has **Continue writing** and **Discard**.

## Sending, undo send and send later

**Send** (`Ctrl+Enter`) puts the message in the outbox. It waits a few
seconds before it goes out (10 by default, see `undo-send-delay` in the
[accounts reference](accounts-reference.md#undo-send)); the status bar
counts down. Click **Undo** to take it back into the composer. Then the
status bar shows "Sending…" and "Message sent.", or why it failed.

**Send later…** shows a date and time field; **Schedule** sends the message
then. Nail must be running at that time.

## Copies in Sent

After a message went out, Nail puts a copy into the account's Sent folder
at once, and then on the server. If the mail server cannot be reached
right then (you are offline, or only the outgoing server works), the copy
waits and is uploaded as soon as the account is back online, like every
other change you make. Gmail and Outlook.com file sent mail themselves, so
Nail does not add a second copy there; `save-sent-copy` in the
[accounts reference](accounts-reference.md) overrides this per account.
POP3 accounts keep the copy in their local Sent folder. [Filter
rules](rules.md) with `trigger="on-send"` run on this copy.

Replying marks the original message as answered.

## The outbox

**Outbox** (in the command palette, as a favorite of the folder pane if you add it, or `g` then `o`) lists messages waiting to be
sent: scheduled ones, ones in their undo-send delay, and ones that failed.
**Edit** takes a message back into the composer; **Send now** sends it at
once, for example after a failure has been fixed. Below them, **Drafts**
lists the messages you saved but have not sent.

## Keyboard

| Keys | In the composer |
| --- | --- |
| `Ctrl+Enter` | Send |
| `Ctrl+S` | Save the draft |
| `Ctrl+Shift+A` | Attach a file |
| `Ctrl+B`, `Ctrl+I`, `Ctrl+U` | Bold, italic, underline (HTML) |
| `Esc` | Close the window (the message is kept) |

The composer's actions can be put on toolbars and keys (see the [layout
reference](layout-reference.md#actions)): `composer.set-mode` with
`"plain"`, `"html"` or `"markdown"`, `composer.toggle-preview`,
`composer.toggle-source` and `composer.attach`.
