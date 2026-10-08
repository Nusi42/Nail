# Nail — English texts of the shell UI (M5b WP161): the navigation rail,
# view switching, badges, and Calendar / Contacts / Tasks as full views.
# Texts named by the contract (view labels, rail label) are in
# core-shell.ftl.

## Navigation rail (widget `nav-rail`)

# The app mark at the top of the rail (decorative).
shell-rail-logo = N
# Tooltip of a rail button with its keyboard shortcut.
shell-rail-tooltip = { common-tooltip-shortcut }
# Accessible name of a rail button with an unread badge.
shell-rail-item-unread = { $label }, { $count ->
    [one] one unread message
   *[other] { $count } unread messages
}
# Accessible name of a rail button with a confirmations badge.
shell-rail-item-confirmations = { $label }, { $count ->
    [one] one change waiting for your approval
   *[other] { $count } changes waiting for your approval
}
# The number on a badge.
shell-rail-badge-count = { $count }
# The number on a badge above its cap ($count is the cap).
shell-rail-badge-many = { $count }+
# The account avatar at the bottom of the rail.
shell-rail-account = Account: { $name }

## View host (widget `view-host`)

# Accessible name of a view's region in the host ($view = the view's label).
shell-view-region = { $view } view

# A view whose window cannot be shown (missing, or hosting views itself).
shell-view-unavailable = { $view } cannot be shown: its window is missing in layout.kdl or contains a view host itself.
