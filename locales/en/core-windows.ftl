# Nail — English texts of the M5c window manager named by the contract
# (ADR 0116): snapped halves, window instances, window-state and pop-out
# issues, pop-out changes.
#
# The ids named by nail_api::windows are pre-filled; nail-api/tests/catalog.rs
# checks them. The UI packages' own texts are in windows.ftl (WP171, the
# window manager), composer.ftl (WP172) and popout.ftl (WP173). Errors of
# `window` node properties in layout.kdl are in core.ftl
# (config-window-size-invalid, config-window-frame-*).

## Snapped halves (nail_api::windows::WindowSnap)

window-snap-left = Left half
window-snap-right = Right half

## Instances (nail_api::windows::WindowInstances)

window-instances-single = One window
window-instances-multiple = Several windows

## Remembered window state (nail_api::windows::WindowStateIssue)

window-state-issue-invalid-window = This is not a window whose position can be remembered.
window-state-issue-invalid-geometry = This window position or size cannot be remembered.
window-state-issue-too-many-windows = Too many window positions are remembered already.

## Pop-out windows (nail_api::windows::PopOutChange / PopOutIssue)

pop-out-change-opened = Opened in its own window
pop-out-change-moved = Window moved
pop-out-change-focused = Window brought to the front
pop-out-change-closed = Window closed
pop-out-change-returned = Window returned to the main window
pop-out-issue-invalid-window = This window cannot be opened on its own.
pop-out-issue-too-many = Too many windows are open on their own already.
pop-out-issue-too-large = This window cannot be opened on its own (too much data).
pop-out-issue-not-open = This window is not open on its own.
