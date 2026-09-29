# Writing mail

## The composer

**New message** (or `c`) opens the composer; **Reply**, **Reply all** and
**Forward** (`r`, `a`, `f`) open it filled in from the selected message.
In this version the composer writes plain text; formatted (HTML) and
Markdown mail come in a later version.

- **From** chooses the identity you send as. New messages start with the
  default identity of the selected account (see [Accounts](accounts.md));
  replies use the identity the message was sent to. Your signature is
  added below a `-- ` line.
- **To**, **Cc**, **Bcc**: several addresses are separated by commas, as
  `ada@example.org` or `Ada Lovelace <ada@example.org>`. **Add Cc** and
  **Add Bcc** show those fields.
- Replies quote the original below "On …, … wrote:"; forwards include the
  original headers and text.

`Esc` or the × button closes the composer without losing the message: **New
message** continues it. **Save draft** (`Ctrl+S`) keeps it as a draft;
**Discard** throws it away.

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
on the server. Gmail and Outlook.com file sent mail themselves, so Nail
does not add a second copy there; `save-sent-copy` in the
[accounts reference](accounts-reference.md) overrides this per account.
POP3 accounts keep the copy in their local Sent folder.

Replying marks the original message as answered.

## The outbox

**Outbox** in the toolbar (or `g` then `o`) lists messages waiting to be
sent: scheduled ones, ones in their undo-send delay, and ones that failed.
**Edit** takes a message back into the composer; **Send now** sends it at
once, for example after a failure has been fixed.
