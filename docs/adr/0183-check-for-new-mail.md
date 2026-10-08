# ADR 0183: Check for new mail beside New message, F5 / Ctrl+R

Status: accepted · 2026-10-08 · review finding (desktop test of 4290474)

## Context

Requirement: a button that fetches new mail from the servers for every enabled account, right of
"New message", and F5 / Ctrl+R doing that instead of reloading the WebView. The core already has the
request (`SyncNow { account_id: null }` = every account) and the action `sync.now` ("Check for new mail",
`refresh-cw`, F5). But the action synced only the active account, nothing in the default layout showed it
(the main toolbar is gone since M5b), and F5 in a text field, a window or a dialog reached no binding, so the
WebView reloaded.

## Decisions

- `folder-tree` option `check-mail` (default `#true`): an icon-only button beside New message in
  `folder-tree.header` (parts `folder-tree.check-mail` / `.icon`), label and tooltip from the action
  (`sync-action-now`), `aria-busy` and `data-state="busy"` while the request runs or the last `SyncProgress`
  is unfinished (the icon turns; reduced motion stops it), disabled while the device is offline
  (`navigator.onLine`, `state/online.ts`). A failure shows in the status bar (`SyncFailed`, as before).
- `sync.now` checks every account; chosen from a folder pane menu (an account's heading, a folder) it checks
  that account (`contextTarget.accountId`). It stays placeable in any toolbar or menu by its id.
- Keys: `ctrl+r` is bound to `sync.now` next to `f5`. `RELOAD_KEYS` (F5, Shift+F5, Ctrl+F5, Ctrl+R,
  Ctrl+Shift+R) never reach the browser: wherever the focus is (fields, windows, dialogs) their binding runs,
  or nothing.

## Consequences

The pre-M5b behaviour "sync the active account" is gone; the tests that expected it were updated. E2E: the
POP3 account of `demo-multi` (no IDLE) receives a message only when the button, Ctrl+R or F5 asks for it.
