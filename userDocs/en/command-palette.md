# The command palette

The command palette runs anything Nail can do — reply, archive, open the
calendar, import from Thunderbird, print, check for updates, the commands
of your plug-ins — by typing a few letters. You never need the mouse.

## Opening it

Press **Ctrl+K** or **Ctrl+Shift+P** (press it again to close the palette),
or click **Command palette** on the navigation rail (the ⌘ button above the
confirmations).
Both are ordinary key bindings: change or add your own in `layout.kdl`
(see the [layout reference](layout-reference.md)), for example

```kdl
keys {
    global "ctrl+space" "palette.open"
}
```

## Finding an action

Type what you want to do. Every word you type must appear in the action's
name or its search words, but the letters do not have to be next to each
other: `rep al` finds **Reply to all**, `arch` finds **Archive**, `pdf`
finds **Save the message as PDF**. Upper and lower case and accents do not
matter. The letters that matched are highlighted.

Each row shows

- the action's name (your own label from `layout.kdl` if you gave one),
- **Recent** for actions you ran from the palette lately,
- **Plug-in** for commands of installed plug-ins,
- the keys bound to it, so you learn the shortcut for next time.

Recently used actions come first when nothing is typed. **Forget recent
actions** at the bottom of the palette clears that list.

## Running it

| Key | What it does |
| --- | --- |
| ↓ / ↑ | Next / previous action (wraps around) |
| Page Down / Page Up | Eight rows down / up |
| Ctrl+Home / Ctrl+End | First / last action |
| Enter | Run the highlighted action |
| Esc | Close the palette |

The palette closes before the action runs, so the action works on what you
were looking at — the selected message, the open draft, the calendar.

## Actions that do not fit right now

Actions that need something you do not have at the moment — **Reply**
without a selected message, **Send** without a draft, calendar actions
outside the calendar — are still listed, at the end and dimmed, marked
**Not available here**. Choosing one tells you where it works instead of
doing nothing silently.

## Your own actions

Aliases you define in the `actions` section of `layout.kdl` appear in the
palette with their label, and so do the commands plug-ins add. Hidden from
the palette are only actions that need an argument to make sense (such as
"open the window named …"); their aliases are listed instead.
