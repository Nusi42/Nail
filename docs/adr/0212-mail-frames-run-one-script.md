# 0212 — Mail frames run one script in an opaque origin

Status: accepted (2026-11-04). Replaces the local frame's design of ADR 0164
(same origin, no scripts) and the reader-side link watching of ADR 0206.

## Context

HTML mail renders in an iframe. Since ADR 0164 the local frame (`srcdoc`:
the desktop app, `nail serve` on loopback) had `sandbox="allow-same-origin"`
and no scripts; the reader read the document's height and listened on the
document for link hovers and clicks (ADR 0206) and for the right-click that
opens the text menu (ADR 0184). The remote UI's frame (sandbox origin) ran
one hash-pinned script instead, because the reader cannot reach into it.

On macOS, links in HTML mail did nothing. WebKit (WKWebView on macOS,
WebKitGTK on Linux) never runs an event listener whose target document sits
in a frame that may not run scripts, even a listener the parent added.
WebView2 and Chromium run it. A minimal page shows it: Chromium reports
`over` and `click`, WebKit neither (a `ResizeObserver` works in both). So
on macOS and Linux no click reached the reader (no status bar, no warning,
no `OpenLink`), and the text menu over mail never opened. ADR 0211 did not
touch this path; it only made a failing `open` visible.

## Decision

- **Every mail frame is sandboxed with `allow-scripts` and never
  `allow-same-origin`**: an opaque origin, so nothing in the document can
  reach the app's DOM, storage or IPC.
- **One script for both frames**, `FRAME_SCRIPT` in
  `crates/nail-api/src/mail_frame.rs` (moved from `nail-server`), exported to
  TypeScript with its CSP source (`'sha256-…'`, `FRAME_SCRIPT_SOURCE`). It
  posts the height, the hovered or clicked link and, new, a context menu
  request with the point, the button and the selected text
  (`FRAME_MENU_MESSAGE`). It accepts one message, only from its parent
  window and origin: `FRAME_SELECT_ALL_MESSAGE` (the text menu's "Select
  all").
- **The parent origin comes from the document**: its root carries
  `data-nail-parent="<origin>"` (`FRAME_PARENT_ATTRIBUTE`), written before the
  mail's markup by `readerDocument` (this window's origin) or the sandbox
  route (the checked `parent` parameter). `location.ancestorOrigins` would
  do for the local frame but does not exist in Firefox (web mode).
- **The script runs only by its hash**: the reader document's own CSP has
  `script-src <hash>`, and since a `srcdoc` document inherits the app's
  policy, the app's `script-src` names the hash too (`app_csp` in
  `nail-server`; `tauri.conf.json`, pinned by a test in the shell). Mail
  scripts are stripped by the sanitizer and blocked by the hash.
- **The reader** (`MailFrame.tsx`) handles both frames with one message
  hook; a menu request becomes a bubbling `FRAME_MENU_EVENT` on the
  `<iframe>`, and the menu host opens the text menu with the frame's text:
  Copy writes it with `navigator.clipboard.writeText`, Select all posts to
  the frame. The remote UI gets Nail's text menu over mail with it.
- **WebKit in the e2e suite**: a Playwright `webkit` project runs the specs
  that depend on the engine (links, frame heights, remote content, the text
  menu) on Windows, so the macOS / Linux behaviour is tested without a Mac.

## Consequences

- Links, the status bar, the link warnings and the text menu over mail work
  in WebKit as in WebView2; the frame height is posted the same way.
- The local document is no longer readable by the reader: everything it
  needs comes through the script's messages. A raw document past the
  sanitizer still cannot run anything: the inherited app policy allows only
  the frame script, `frame-src` keeps a meta refresh from loading another
  page into the frame, and the opaque origin keeps the app out of reach.
- Changing one byte of the script changes its hash: the server's and the
  reader's policies follow automatically, `tauri.conf.json` by hand (the
  shell's test names the expected value).
- Keyboard copying inside the frame (Ctrl+C) stays the engine's own.
