# 0211 — Opening attachments, file dialogs off Windows, links in plain text

Status: accepted (2026-11-02)

## Context

Three problems of the desktop app, found on macOS:

- A click on an attachment did nothing: the card had a Save button and no way
  to open the file.
- "Save as" for attachments did not work. `DesktopHost::pick_path` existed on
  Windows only (ADR 0202); elsewhere `PickPath` answered `unsupported`, the UI
  fell back to a blob download (`<a download>`), and the webview of macOS and
  Linux ignores that.
- Clicking links did nothing. HTML mail has answered clicks with `OpenLink`
  since ADR 0206 (the core hands `http:` / `https:` / `tel:` to the desktop
  host, which runs `open` / `xdg-open` off Windows). The text of a mail
  without an HTML part is a `<pre>` with no anchors, so nothing in it was
  clickable, and a failing `open` was not reported.

## Decision

- **File dialogs through `rfd`** (MIT / Apache-2.0, 0.16.x; `default-features =
  false`, feature `gtk3`) as the `cfg(not(windows))` implementation of
  `DesktopHost::pick_path`. macOS: `NSOpenPanel` / `NSSavePanel` (objc2, no
  system library beyond AppKit); Linux: GTK 3, which Tauri's webview already
  links, so no portal, D-Bus or async runtime is added. The dialog runs on a
  thread of its own (`rfd` moves itself to the main thread on macOS) and is
  owned by the main window. `dialog::spec` (texts, filters, start folder, file
  name) is shared; `dialog::show` is per system. Test runs still cancel the
  pick. Windows keeps its common item dialog. The cargo-deny graph lists
  Windows and Linux targets; `rfd`'s licences are on the allow list and it
  brings no GPL-family crate. PDF printing stays Windows only.
- **Open an attachment** (`Request::OpenAttachment { message_id, index }` →
  `Done`, desktop only like `SaveAttachment`, so `DesktopOnly` for remote
  sessions; not an MCP tool). The core decodes the attachment (encrypted mail
  in memory as before), writes it to `<data dir>/opened-attachments/<unique>/
  <sanitized name>` and calls `DesktopHost::open_file(path)`: `ShellExecuteW`
  on Windows, `open` on macOS, `xdg-open` elsewhere; only an absolute path of
  an existing file is accepted. Every open gets a subfolder of its own; on
  Unix the folders are `0700` and the file `0600`; the names go through the
  portable file-name sanitizer (no separators, reserved characters or device
  names, at most 120 characters, `attachment` without a name). The whole
  folder is removed when the service starts and when it shuts down, and each
  open prunes folders older than a day (a file a viewer still holds is kept).
  Without a desktop host the answer is `unsupported` and the UI downloads the
  file as before (web mode, remote UI). The host answers failures with the
  localized `attachment-open-failed`. Test runs of the desktop app launch
  nothing and record the paths for the `opened-files` test control.
- **The UI.** The card's name is a button, the badge a pointer shortcut of it
  (`tabindex="-1"`, `aria-hidden`), and an explicit Open icon button joins
  Save; the status line gains opening / opened / could not be opened. An
  attachment the core flagged (`MessageWarning::DangerousAttachment`:
  programs, macro documents, disguised names, archives, shortcuts) first asks
  in the modal confirmation `mail-open-attachment` ("Open anyway" / "Cancel",
  Cancel focused), built from the shared `MailConfirm` of the link
  confirmation; nothing opens without the answer.
- **Links in plain text.** The reader makes the URLs of a plain-text body
  anchors (`ui/src/widgets/plainTextLinks.ts`, `LinkedText.tsx`): only the
  schemes of `nail_api::links` (`EXTERNAL_LINK_SCHEMES`, `MAILTO_SCHEME`, now
  generated constants), trailing punctuation and unbalanced brackets cut off.
  A click, Enter and a middle click are cancelled and reported like a link of
  an HTML mail, so they go through `OpenLink`, the status overlay and the
  same confirmation. The core numbers only the links of an HTML document;
  plain-text links have number -1 and are never flagged (what is shown is the
  target).
- **`open` / `xdg-open` errors are reported.** macOS waits for `open` (it
  returns at once) and reports a non-zero exit with its message, so a missing
  handler is a toast and `link-open-failed`, not silence; `xdg-open` is
  started and reaped on a thread (it may stay until the program quits).

## Consequences

- Save as, Open and links work the same on Windows, macOS and Linux, as far
  as they could be checked: nothing here ran on macOS (WSL2 Linux builds and
  lints the non-Windows code; the macOS dialog and `open` paths are
  unverified runtime behaviour).
- ADR 0202's "no native file dialog off Windows" is lifted; PDF printing and
  the LAN address list remain follow-ups.
- Opened attachments are plaintext files in the user's data directory until
  the next start, shutdown or prune; the confirmation for risky types is the
  only guard (the system's own checks, e.g. Gatekeeper, apply on top).
- The blob-download fallback of the UI can still report a download that a
  webview ignored; it is now reached only without a desktop host (browsers
  honour it), so no cheaper signal was added.
- Links in plain text no longer open inside the page: they were text before
  and are anchors that never navigate now.
