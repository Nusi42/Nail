# The Nail desktop app on Windows

This page covers what only the Windows desktop app does: updates and
*About Nail*, the tray and starting with Windows, default apps, mail that
other programs hand to Nail ("Send to → Mail recipient"), and the welcome
screen on the first start. In a browser (remote access, `nail serve`)
these settings are not available.

## The welcome screen

The first time you start Nail it shows a welcome screen with three
things to set up. You can change all of them later in Settings.

- **Open e-mail links with Nail.** Windows lets only you choose which app
  opens e-mail links, calendar links and mail files. *Choose in Default
  apps* opens the right page in Windows Settings. The screen also shows
  what e-mail links open now.
- **Start with Windows.** Nail then starts when you sign in, in the
  notification area without a window, so new mail, rules and reminders
  work from the start.
- **Bring your mail along.** Importing mail, contacts and calendars from
  another mail program comes with a later version of Nail.

*Get started* and *Skip* both close the screen for good. If you close it
with ×, it comes back at the next start. To see it again, use the
*Welcome screen* command.

## Updates and About Nail

*About Nail* shows the version, how Nail was installed (with
`Nail-setup.exe`, by your organization, as a portable copy or as a
development build), for whom (just you or all users of the PC), the
architecture, the update channel, the folder and the WebView2 version.
Below that is the update status.

In **Settings → Updates** you choose:

- **Update channel.** *Stable* or *Beta (early versions)*.
- **Check for updates automatically.** At start and once a day.
- **Download updates in the background.** Otherwise Nail tells you about
  a new version and waits for *Download*.

What the update status can show:

| Status | What you can do |
| --- | --- |
| Up to date / checks automatically | *Check now* |
| Checking for updates… | Wait a moment |
| Version x is available | Read *What's new*, then *Download* |
| Downloading version x… | The bar shows the progress |
| Version x is ready | *Restart to install*: Nail closes, installs and starts again |
| A problem (no connection, invalid signature, …) | *Try again*. An update with an invalid signature is never installed |
| Updates are off | Nothing — see below |

Updates are signed, and Nail checks the signature before it installs
anything. Your mail and settings stay as they are.

**When updates are off.** Nail says why instead of offering the buttons:

- *Your organization installs updates for Nail.* — Nail was installed
  from the MSI package. Your IT department updates it.
- *Updates are turned off by your organization's policy.*
- *The portable copy is updated by hand* — download the new version and
  replace the folder.
- *Development builds do not update themselves.*

An organization can also fix the update channel; the channel list is then
locked and says so.

## Tray and startup

In **Settings → Tray and startup**:

- **Keep Nail running in the notification area** when you close the
  window. Mail sync, rules, reminders and connected AI apps keep working.
  Choose *Quit* in the tray icon's menu to end Nail.
- **Start Nail with Windows.** Nail starts when you sign in to Windows.
- **Start in the notification area, without a window.** Only when Nail
  starts with Windows; open it from the tray icon.
- **Allow "nail plugin dev" to load plug-ins into this app.** For people
  who write plug-ins: `nail plugin dev` can then load a plug-in from a
  folder into the running app. Only programs on this PC that can read
  your Nail data folder can connect. While it is on, the address it
  listens on is shown. Leave it off otherwise.

## Default apps

**Settings → Default apps** lists what Nail can open — e-mail links
(`mailto:`), calendar links (`webcal:`, `webcals:`), e-mail files
(`.eml`), calendar files (`.ics`), contact files (`.vcf`) — and whether
Nail is the mail program other programs use for "Send to → Mail
recipient". For each one it shows *Nail is the default*, *Another app is
the default* or *Windows does not tell* (Windows does not reveal this for
every kind).

Windows never lets an app make itself the default. *Choose in Default
apps* opens Windows Settings on Nail's page; choose Nail there and come
back. Nail reads the state again when you return; *Check again* does it
by hand.

If Nail says it is not listed in Windows Settings, or that the component
for "Send to → Mail recipient" is not installed, run the installer again
to repair or add it.

## Mail from other programs ("Send to → Mail recipient")

When Nail is the system's mail program, "Send to → Mail recipient" in
File Explorer, "Send as attachment" in Office and the "e-mail this"
commands of other programs open Nail's composer with the recipients,
subject, text and files the program passed. A note above the message
names the program, for example *WINWORD.EXE prepared this message. Check
it, then press Send.*

**Nail never sends for another program.** Some programs ask the mail
program to send without showing a window. Nail opens the composer
anyway, and the message goes only when you press *Send*. Programs that
expect a silent send see the composer instead. The message is saved as a
draft, so closing the composer keeps it in *Drafts*.

Nail reads the files when the program hands them over, so it does not
matter if the program deletes its temporary files right away. Each file
must fit the composer's attachment limit. If Nail has no e-mail account
yet, the program is told to set one up first.
