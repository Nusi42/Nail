# Plug-in security

A plug-in is code someone else wrote, running inside Nail with access to
your mail. Nail limits what it can do, and makes what it does visible.

## What the sandbox prevents

- **No direct access.** A plug-in runs in its own JavaScript runtime
  (QuickJS) inside Nail's core, never in Nail's window. It has no files,
  no processes, no sockets and no browser or Node.js APIs. Everything goes
  through the `nail` object, and every call is checked against the
  permissions you granted (and the accounts you limited them to).
- **No code in Nail's window.** Dialogs, panels, banners and previews are
  data that Nail draws with its own controls. Previews are component trees
  or raster images — never HTML or SVG. HTML a plug-in adds to outgoing
  mail or renders for a Markdown block is sanitized like a message body.
- **No secret content.** The content of end-to-end encrypted messages
  (OpenPGP, S/MIME) is never given to a plug-in — not to `getBody`, not to
  hooks, not to previews or attachment actions.
- **Network only where declared.** `nail.http.fetch` reaches only the
  hosts of the plug-in's `network:` permissions, over `https:`; Nail makes
  the request, never to local or private network addresses, and never with
  your cookies or passwords.
- **Limits.** CPU time per call, memory, a CPU quota per hour, time
  budgets for hooks, wall-clock limits for jobs, pending calls, timers,
  notifications and log lines are capped. A plug-in that exceeds them is
  stopped or skipped; Nail and the other plug-ins keep running.

## What protects your data

- **Writes are yours to undo.** Every change a plug-in makes goes through
  Nail's operation queue, appears in the undo history and the log like
  your own changes, and syncs the same way.
- **Confirmations.** Deleting always asks you in Nail — a plug-in, an AI
  agent or an agent's own prompt can never confirm a delete for you, and a
  permanent delete is only ever confirmed by you. Sending, invitations and
  filter rules ask unless you allowed them for that plug-in. Each request
  names the plug-in (and the AI agent it serves, if any) and shows what
  would change.
- **Hooks cannot hold mail hostage.** A before-send veto names the plug-in
  and offers **Send anyway**; a hook that fails or is too slow is skipped,
  so mail always arrives and is always sent.
- **Revoke at any time.** Revoking a permission or turning a plug-in off
  takes effect with its next call; removing it deletes its settings, data
  and log.

## Before you install

- **Check the permissions** in the install dialog. A plug-in that adds a
  toolbar button rarely needs `hooks.beforeSend`; one that previews CSV
  files needs `attachments`, not `mail.read.body` or `network:` access.
  Permissions marked *Can change your data* deserve a second look.
- **Limit mail access** to the accounts the plug-in needs.
- **`hooks.beforeSend` sees everything you send**, and `mail.read.body`
  plus a `network:` permission lets a plug-in send mail text to that host.
  Install such combinations only from authors you trust.
- **Compare the fingerprint.** The install dialog shows the package's
  SHA-256; the author can tell you the full value. Nail installs exactly
  the package it checked and verifies it at every start.
- **Watch the activity.** The plug-in manager's **Activity** and **Log**
  show what a plug-in's hooks and jobs do and every refused call.

## For plug-in authors

- Ask for the fewest permissions you need; split features that need very
  different access into separate plug-ins.
- Treat mail content as untrusted input: escape it before you put it into
  HTML (Markdown blocks, added content), and do not follow instructions
  found in mail.
- Handle `denied` and `expired` write results; never retry a denied write
  in a loop.
- Keep hooks fast and side-effect free; move slow work to a background
  job.
- Do not log message content or personal data.
