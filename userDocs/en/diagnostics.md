# Diagnostics for support

When something does not work and support asks for details, Nail can write
a **diagnostics bundle**: one zip file with logs and settings. You decide
what goes in, you see every file before it is written, and Nail never
sends it anywhere — you pass the file on yourself.

## Creating a bundle

Open **Settings → Data & backup** and choose **Create diagnostics bundle…**,
or run **Create a diagnostics bundle…** from the
[command palette](command-palette.md).

1. **Choose what to include.** By default: system information (Nail,
   Windows and WebView2 versions), recent logs, your configuration files,
   account states, database statistics, pending changes and plug-ins.
2. **Replace mail addresses and remove subjects** is on by default: logs,
   settings and account states then contain placeholders such as
   `<address-1>` instead of addresses, and no subjects or names. Support can
   still follow what happened.
3. **Show what will be included.** Nail lists exactly the files the zip
   will contain, each with its size, what was removed from it, and — under
   **Show text** — the beginning of its content. Read it if you want to
   be sure.
4. **Save bundle.** Type a path or choose **Browse…**. Nail shows where the
   file is, its size and its checksum (SHA-256), which you can quote to
   support so they know they got the same file.

If you change any option after looking at the list, Nail asks you to look
at the new list first.

## What is never included

Passwords, sign-in tokens, keys and keychain names are always removed, in
every file. Your user name in file paths is replaced.

## Including messages

Some problems can only be understood with the message that causes them.
Mail is never included unless you ask for it:

- tick **Headers of chosen messages** or **Chosen messages (full
  content)**,
- then tick the messages to include — the message selected in the message
  list when you opened the dialog is offered.

These messages go into the bundle unchanged, with their addresses and
content (full content includes attachments). At most 20 messages fit in
one bundle.
