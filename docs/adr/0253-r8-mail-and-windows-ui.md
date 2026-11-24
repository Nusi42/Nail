# 0253 — R8: mail, composer, rules, settings and windows UI

Status: accepted (2026-11-24)

## Context

The R8 contract (ADR 0251) gave the mail and windows UI package the backlog rows of its features: favorites
that open windows, the searched folder, several unsaved composers, a background composer's sign / encrypt
report, the composer preview's height, rules on a whole list (UI side), the shared texts of a failed Retry /
Dismiss, layout presets, the dialog footer, settings drafts, pop-out key bindings, the toolbar customizer, the
rail's account avatar, and four rows to check first (monospace text areas, "Run selected rules" from the menu,
menus and the settings page, the search hint key).

## Decision

### Scope decisions

1. **Favorites as views.** A favorite that opens a window names that window (`FavoriteTarget` `action` with
   `window`; the Outbox's window is the layout's `outbox.open` alias, pinned by a test). While that window is
   the topmost one (`favoriteWindowView`), the favorite is the one selected item of the folder pane: no folder,
   favorite search or smart folder is marked then. Unread and Flagged search `NOT folder:trash NOT
   folder:junk` (`FAVORITE_SEARCH_EXCLUDES`; `folder:` matches a folder's role in the core).
2. **Searched folder.** The folder a search runs in (`SearchState.folderId`) is marked: `data-state="searched"`,
   a search icon (`folder-tree.item.searched`) and the row's description "The search runs in this folder" (its
   name stays the folder's).
3. **Several unsaved windows.** When several guarded windows with unsaved changes leave in one store update,
   the close guard asks once (`SEVERAL_GUARD`, the question's `several`): the prompt lists their names; Save
   saves each in turn and stops at the first that fails (that window comes to the front); Don't save drops
   all; when one cannot be saved only Discard and Keep editing are offered. The shell's view switch without a
   view host closes every window in one update.
4. **Background composer report.** Every composer's sign / encrypt widget reports for its own window
   (`composerCryptoStore.setReportOf`); a focus change makes the kept report the focused one at once
   (`activate`), and a closed composer's report is forgotten. The report follows a background composer's
   recipients; nothing waits for the focus.
5. **Preview height.** The Markdown preview is a frame document of Nail's own (`framedDocument`: the reader's
   CSP and the frame script, `MAIL_FRAME_SANDBOX`, opaque origin) and is sized by the height it reports
   (`useLocalFrame`, shared with the reader's local frame), between the editors' minimum and
   `PREVIEW_MAX_HEIGHT` (32rem); a longer preview scrolls inside it. Where the page's policy does not run the
   frame script (a remote browser), the frame keeps its default height.
6. **Rules on a whole list.** After select all, the menus' "Run rule" sends `RunRules` with `list` (the list
   target of the selection, `wholeListOf`) and no message ids; the run toast's Undo undoes it as before. The
   rules window's "Run on: selected messages" is unavailable while the whole list is selected (its Test and the
   count before a destructive run name messages one by one; `rules-target-whole-list` says where to run it).
7. **Retry / Dismiss.** A failed bulk Retry / Dismiss shows `status-retry-failed` / `status-dismiss-failed`
   (with the reason) as an alert next to the failure, which keeps its own reason.
8. **Layout presets.** The layout card's preset is a settings draft: chosen, it shows pressed; the bar's Save
   applies it (`apply-preset`), Discard forgets it. Reset to default and Customize layout stay actions.
9. **Dialog footer.** `DialogFooter.secondary` takes one button or several; several keep the secondary place
   as one group, in the given order on Windows and Linux, mirrored on macOS (`secondaryOrder`). Plug-in
   dialogs pass all their secondary buttons.
10. **Settings drafts.** Drafts live in a draft scope (`DraftScope`): the settings page uses the UI store's,
    `SettingsDraftScope` gives a window its own (the plug-in manager window, one per mount). The shown
    section's navigation entry shows a dot while it has unsaved changes (its description "Unsaved changes").
    The contact groups window's "New group" name stays unguarded (the window has one close guard, the group
    editor's; the PIM package's file) — the backlog row keeps that part.
11. **Pop-out key bindings.** Proposed defaults (window context, conflict-free in the default layout):
    `ctrl+shift+o` → `popout.open`, `ctrl+shift+m` → `popout.return`. They belong in `defaults/layout.kdl`
    (and its fixture), which the integration changes; the key binding editor lists them from the layout.
12. **Toolbar customizer.** The available actions are no longer cut at 30: they are grouped by where they
    apply (`AVAILABLE_GROUPS`: spacing, the palette availabilities, plug-ins) as ARIA groups of one listbox;
    the search narrows every group.
13. **Rail.** The account avatar is a button (`nav-rail.account`, in the rail's roving focus) that opens the
    account menu (`accountMenuItems`: the accounts, "All accounts" first, the shown one checked; Account
    settings; Add account). Choosing an account shows it in the view that lists folders
    (`SHOW_ACCOUNT_ACTION`). The avatar keeps its size, so the rail's baselines stay.
14. **Menu buttons.** The toolbar's More and the rail's avatar share `useMenuButton` (`features/menus`): a
    second press closes the menu (the menu leaves presses on its `owner` button to the button), the open key,
    the focus back on close. More's menu ends at its button's end edge (`menuPosition`, `alignRight`).
15. **Checked rows.** Monospace text areas: done — every text area inherits the UI face, monospace only for
    code (HTML source, rules text, the KDL to merge); a test pins it. "Run selected rules" from the menu: the
    rule list has no menu of its own and the footer's Run already runs the selection (R3); no second entry
    point. Menus and the settings page: "Pair a device" shows once and the More menu is now anchored; the one
    widget that repeated its section's title (Appearance) hides its heading visually on the settings page. The
    search hint key: `/` is right — Ctrl+K is the palette (pinned by a test).

## Consequences

- Playwright: `r8-p2-mail.spec.ts`, `r8-p2-composer.spec.ts`, `r8-p2-settings.spec.ts`,
  `r8-p2-windows.spec.ts`. The whole-list rules scenario needs the core's list form (P3); until then the core
  refuses the request.
- `m5d.spec.ts` saves the preset with the settings bar.
