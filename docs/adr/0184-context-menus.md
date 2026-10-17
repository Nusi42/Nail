# ADR 0184: Nail's context menu or none — never the browser's

Status: accepted · 2026-10-08 · maintainer finding (desktop test of 4290474); amends ADR 0043

## Context

Right-clicking an empty part of the folder pane showed a menu that did not belong to Nail. It was WebView2's
own menu (Back, Refresh, Save as, Print, Inspect): `ContextMenuHost` cancelled a right-click only where the
layout had a menu for the part under the pointer (a row, a folder, an account heading, the reader, the rail)
and left text fields to the browser on purpose. The maintainer's rule: a place has Nail's own menu, or a
right-click does nothing.

## Decisions

- **One handler.** `ContextMenuHost`'s document listener cancels every `contextmenu` event (a part's own
  handler, the toolbar's, may open its menu first) and opens Nail's menu for the place, or nothing. Mail
  documents in their frames (`srcdoc`, same origin) get a listener on their own document when they load
  (`load` captured on the page); it opens the text menu at the frame's position. The remote UI's mail frames
  (opaque origin) cannot be reached from the page; the desktop app never uses them.
- **The text menu.** Context part `text` (contract `CONTEXT_MENU_PARTS`, UI mirror), `menu "context:text"` in
  `defaults/layout.kdl`: `edit.cut`, `edit.copy`, `edit.paste`, separator, `edit.select-all` (icons `scissors`,
  `copy`, `clipboard-paste`, `text-select`; labels `edit-action-*` in `menus.ftl`). Its targets: a text field or
  editor; selected text under the pointer; else the part's own menu; else selectable text (ADR 0182). The
  selection is saved when the menu opens and put back before the command. Items that do not apply are hidden
  (Cut and Paste need a writable field, Cut and Copy a selection, Paste the async clipboard). The menu shows the
  platform keys (`EDIT_SHORTCUTS`; not bindings — fields handle them natively). Cut, Copy and Paste use
  `execCommand` (the only API that keeps the field's undo history and the editors' input events); Paste reads
  `navigator.clipboard`, which the desktop windows allow without a prompt (`enable_clipboard_access`).
- **The folder pane** around its folders and accounts: context part `folder-tree`,
  `menu "context:folder-tree"` = Check for new mail, New message, Add account. Other empty places (the list's
  background, headings, the status bar, window bodies) have no menu: a right-click there does nothing.
- **Desktop.** Every webview (main window, pop-outs, print windows) gets
  `AreDefaultContextMenusEnabled = false` from `tauri::Builder::on_page_load` (`webview_menus.rs`); the value
  WebView2 reports back is the desktop test's `nail_test_shell "default-context-menus"`. The developer tools
  stay on F12 in debug builds only (Tauri's default).

## Consequences

Tests: `menus.test.tsx` / `textMenu.test.tsx` (targets, the text menu's items, Copy / Cut / Paste / Select all
on the saved selection, the folder pane menu, nothing on chrome), `e2e/tests/app-chrome.spec.ts` (every
right-click across the views is cancelled; the folder pane, field, mail frame and rail menus) and
`desktop-m5c.spec.ts` (WebView2 reports the default menus off for every window).
