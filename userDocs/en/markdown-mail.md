# Markdown mail

In Markdown mode you write plain text with light markup, and your
recipients get a formatted message. Nail turns the Markdown into HTML
itself, so the preview next to the text is exactly what will be sent.
Choose **Markdown** under **Format** in the composer, or make it the
default for an identity with `composer="markdown"` (see the [accounts
reference](accounts-reference.md#identities)).

## What you can write

Nail understands CommonMark with the GitHub additions:

| You write | Recipients see |
| --- | --- |
| `**bold**`, `*italic*`, `~~struck~~` | **bold**, *italic*, ~~struck~~ |
| `# Heading`, `## Smaller heading` | Headings |
| `- item` or `1. item` | Bulleted and numbered lists |
| `- [ ] open`, `- [x] done` | A task list with check boxes |
| `> quoted` | A quote block |
| `[text](https://example.org)`, `https://example.org` | Links (plain addresses become links by themselves) |
| `` `code` `` and fenced blocks with <code>```rust</code> | Code, with syntax colours for the named language |
| `\| a \| b \|` rows under a `\|---\|---\|` line | A table |
| `text[^1]` and `[^1]: note` | A footnote at the end |
| a line ending in `\` | A line break |

Raw HTML is allowed, but everything that could run code or load something
on the recipient's side is removed before sending (the same cleaning as in
HTML mode).

The signature after the `-- ` line keeps its line breaks.

## What is sent

A Markdown message is sent as:

- a **plain-text** version: your Markdown as you wrote it, which reads well
  in any mail program;
- an **HTML** version: the rendered message, with the stylesheet built in
  (`style` attributes), so it looks the same in clients that ignore
  stylesheets;
- optionally the **Markdown source** as a separate `text/markdown` part
  ("Also send the Markdown source" in the composer), for recipients whose
  mail program can show or edit Markdown.

Attachments and quoted replies work as in the other modes (see [Writing
mail](composer.md)). A reply in Markdown quotes the original with `>`.

## Switching modes

Plain text becomes Markdown unchanged, and Markdown becomes HTML without
losing anything. Switching from Markdown to plain text keeps your text but
drops the formatting; Nail asks before doing that.

## How it looks

Each identity can set its own look with `mail-style` in `accounts.kdl`:

```kdl
account "work" email="ada@work.example" {
    imap host="imap.work.example"
    identity "ada@work.example" name="Ada Lovelace" composer="markdown" {
        mail-style css="body { font-family: Georgia, serif; font-size: 15px; } blockquote { color: #555; }" code-theme="Solarized (light)"
    }
}
```

- `css` replaces Nail's built-in stylesheet (a plain, readable one with
  light table borders and grey quotes). It is applied to the HTML version
  of Markdown and HTML messages. Remote files (`@import`, `url(…)` to the
  internet) are not loaded.
- `code-theme` colours code blocks: `InspiredGitHub` (default),
  `Solarized (light)`, `Solarized (dark)`, `base16-ocean.light`,
  `base16-ocean.dark`, `base16-eighties.dark` or `base16-mocha.dark`.

The preview in the composer uses the identity chosen under **From**.
