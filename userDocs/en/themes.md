# Your own themes and the theme editor

Besides the five built-in themes (see [Appearance](appearance.md)), Nail
lets you make themes of your own and change every colour, size and part
style in the **theme editor**, with a live preview. Your themes are files in
the `themes` folder of your configuration folder (`themes/<id>.kdl`), so you
can also share them or edit them by hand (see the
[theme reference](theme-reference.md)).

## Making a theme

Built-in themes stay as they are. To change one, make a copy:

- **Settings → Appearance → New theme**, or `Ctrl+K` → **New theme**: give
  the theme a name and choose the theme it starts from (the one in use is
  preselected). The window shows the file name Nail will use, such as
  `themes/ocean.kdl` for "Ocean": letters without accents, in lower case,
  with hyphens for everything else. With **Use the new theme now** (on by
  default) the new theme replaces the one it was copied from — as the light
  or dark partner of Follow Windows when that is on. **Create** opens the
  new theme in the editor.
- In the theme editor, **Duplicate as new theme** copies the theme shown
  ("Frost copy", `themes/frost-copy.kdl`). When the copied theme was in
  use, the copy takes its place, and the editor continues on the copy.

## The theme editor

Open it with **Edit theme** on the Appearance page, `Ctrl+K` → **Edit
theme**, or the inspector's **Edit style** (below). It is a normal window:
you can keep it open beside the mail view and watch your changes apply.

At the top, **Theme** chooses which theme you edit (built-in themes can be
looked at, not changed), followed by **New theme**, **Undo** and **Redo**.
For one of your themes there are also **Use this theme**, **Duplicate as new
theme**, **Rename** and **Delete**.

### Tokens

Tokens are the named values every part of Nail is styled with. The tabs
group them:

| Tab | Tokens |
| --- | --- |
| **Colours** | backgrounds, text, borders, the accent, states (`color.*`) |
| **Type** | fonts, the type scale and weights |
| **Spacing** | the 4 px spacing grid and control sizes |
| **Corners** | corner radii (`radius.*`) |
| **Motion** | animation durations and easing |
| **More** | everything else the theme defines (avatar colours, shadows, …) |

Each row shows the token's name, where its value comes from — **Base** (the
base theme every theme starts from), **Inherited** (the theme yours extends)
or **This theme** — and the value as it is written in the file. Colours have
a colour picker beside the text field. Type a value and press `Enter`, or
leave the field; values can refer to other tokens (`$color.accent`) and use
CSS colour functions. The button at the end of a row goes back to the
inherited value. **Find a setting** filters the rows.

**Values for** chooses which value you edit: **All modes** (the base value)
or the value used only in dark mode, in High contrast, when printing, or in
the Compact density.

### Parts

The **Parts** tab lists every part that has a style (a part is a named
piece of the interface, such as `toolbar.button` or `message-list.row`; see
[Row templates, parts and keyboard](templates-and-parts.md)). Search with
**Find a part**, or type a part's name under **Part** and click **Add** to
style a part no rule names yet. For the chosen part you see its
properties: the theme's own ones can be changed or removed, inherited ones
can be overridden in this theme, and new properties can be added. **Reset
part** removes the theme's own rule for the part.

### The preview

On the right, a small mail window shows the theme's colours and sizes,
including changes you have not saved yet. Part rules show in the app itself
while the theme is in use.

### Saving and undo

There is no Save button: the editor writes the theme file shortly after you
stop typing (all changes of one pause together) and when you close the
window. Each write is one step for **Undo**, labelled "Undo Edit theme". The
same history holds the choices on the Appearance page (theme, Follow Windows,
density, accent, text size, corner radius), so **Undo** there and in the
editor go back through all of them. The history lasts until Nail quits.

If the theme file was changed outside Nail since the editor read it, nothing
is written: the editor says so and offers **Reload the theme**. Undo then
offers **Undo anyway**.

## Renaming and deleting

- **Rename** changes the theme's name. With **Also rename the file** the
  file gets a matching name too; wherever the theme was in use, or another
  of your themes extends it, Nail follows the new name.
- **Delete** asks first, then removes the file. Where the theme was in use,
  Nail switches to the default theme of its kind (Frost, Graphite or High
  contrast). **Undo** brings the file back.

## Edit style from the inspector

With the [inspector](customization.md#the-inspector) on, click any element
and choose **Edit style**: the theme editor opens on the theme in use, on the
**Parts** tab, at the element's part. When the theme in use is a built-in one,
duplicate it first to change the part.

## What Nail writes

A theme made in the editor is an ordinary theme file:

```kdl
theme "Ocean" extends="frost" {
    tokens {
        color "accent" "#117755"
        color "bg" "#f4fbfa" dark="#0d1b1a"
    }
    part "message-list.row" {
        border-radius "$radius.md"
    }
}
```

Comments and the order of what you wrote by hand are kept. The
[theme reference](theme-reference.md) lists every token and part.
