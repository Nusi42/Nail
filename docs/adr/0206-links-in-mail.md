# 0206 — Links in mail open outside the reader's frame

Status: accepted (2026-10-31)

## Context

The reader shows HTML mail in a sandboxed frame (ADR 0164): a `srcdoc`
frame with `sandbox="allow-same-origin"` and no scripts on the desktop and
in web mode, a frame on the remote UI's sandbox origin with one hash-pinned
script for remote sessions. Nothing handled a click on a link, so the click
navigated the frame itself. A newsletter's ordinary click-tracking link
(`https://…/click` around a `<span>` or an `<img>`) then showed WebView2's
"This content is blocked" page in place of the mail, because the target
forbids framing; a site that allows framing would even have loaded inside
the mail. The user also could not see where a link goes before clicking,
although the core already flags disguised links (`MessageWarning`
`link-mismatch`, `lookalike-domain`, `suspicious-link`, ADR 0032).

## Decision

- **No link navigates a mail frame.** A click, Enter on a focused link and a
  middle click on `closest("a[href]")` are cancelled (`preventDefault`) and
  reported to the reader. Local frames: the reader listens on the frame's
  document (`ui/src/widgets/mailLinks.ts`, `watchLinks`); the sandbox stays
  `allow-same-origin` only, never `allow-scripts`. Remote frames: the
  sandbox route's script (`FRAME_SCRIPT`, formerly the height script) does
  the same and posts `{type: "nail-frame-link", open, href, index}`
  (`FRAME_LINK_MESSAGE`) to the `parent` origin; the reader accepts it only
  from its own frame's window and the sandbox or opaque origin, like the
  height. The script stays pinned by its hash in the CSP; tests pin what it
  may do.
- **The core decides what a link does** (`Request::OpenLink { url }` →
  `Response::LinkOpened { outcome }`, `nail_api::links`, `nail_core::links`):
  - `http:` / `https:` / `tel:` are parsed and normalized. The desktop app
    hands them to the system (`DesktopHost::open_link`: `ShellExecuteW` on
    Windows, `open` / `xdg-open` elsewhere; the shell checks the scheme
    again) and answers `Opened`. Without the desktop shell (web mode) and
    for every remote session the answer is `OpenInBrowser { url }` and the
    UI opens a new tab with `noopener,noreferrer`; a remote session never
    reaches the desktop's browser. The request needs only a session.
  - `mailto:` is decoded by the parser of `mailto:` launches and answered
    `Compose { mailto }`; the UI opens Nail's composer
    (`startMailtoCompose`).
  - Any other scheme or a malformed link is `invalid_request`
    (`link-unsupported`), so no path or other protocol reaches the system.
- **A status overlay shows the target** while a link is hovered or focused:
  at the reader's bottom edge, over the content (a zero-height sticky dock,
  `message-view.link-dock` / `.link-status`), shortened in the middle with
  the full value in the accessible text. Styling is the skin's and any
  theme's (`data-part`, `data-state~="warning"`).
- **Flagged links warn and ask.** The reader maps a link to the core's
  number the way `nail_mime::links` numbers it (the position among the
  document's `a[href]`). A link with a `MessageWarning` shows the warning's
  sentence in the overlay, and a click first opens the modal confirmation
  `mail-open-link` ("Open anyway" / "Cancel", Cancel focused).
- **The warnings banner stays short.** Link warnings of the same kind with
  the same shown and target domain are one entry with their number of links
  (`security-warning-links`); the banner shows `MAX_SHOWN_WARNINGS` (3)
  entries and a "Show all N warnings (M more)" / "Show fewer" button
  (`aria-expanded`); the title keeps the total. A real target is one line
  with a CSS ellipsis (the full address stays in its text and tooltip), and
  the list scrolls past a height. A newsletter whose every link goes
  through a tracker no longer pushes the mail out of view.
- A pending confirmation is keyed by the app store's `setState`, which a
  card's scoped store (`scopedUiStore`) shares, so a link clicked in a card
  of the feed is answered in the app's window.
- Test runs of the desktop app open no browser; they record the links for
  the `opened-links` test control.

## Consequences

- Tracking links, image links and `target`-less links open where the user
  expects; the mail stays in its frame.
- The remote sandbox document's script grows; its hash changes with every
  byte and the CSP follows automatically. It still reads no state and
  posts only to the named parent.
- A new browser tab is opened after the core's answer (a WebSocket round
  trip): within the browser's transient user activation, so popup blockers
  allow it.
- Links in plain-text mail are not clickable today and are not affected.
