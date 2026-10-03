# Appearance: themes, Follow Windows, density

Nail comes with five themes and lets you switch between them without
touching a file. Open **Settings → Appearance**.

## Themes

| Theme | Kind | Look |
| --- | --- | --- |
| **Frost** | light | Cool blue-grey neutrals with an indigo accent. The default. |
| **Sage** | light | Soft green-grey neutrals with a green accent. |
| **Graphite** | dark | Neutral dark greys with a periwinkle accent. The default dark theme. |
| **Nocturne** | dark | Deep blue night tones with a teal accent. |
| **High contrast** | high contrast | Pure black and white with yellow and cyan accents, thick focus rings, visible borders on every control, bigger text and no animations. |

Click a theme in the gallery to use it. That switches **Follow Windows**
off: Nail keeps the theme you picked whatever Windows does.

Every theme meets the WCAG contrast guidelines: at least 4.5:1 for text and
3:1 for outlines and indicators (7:1 for everything in High contrast). Nail
checks this automatically for each theme.

Printing always uses white paper and black text, whichever theme you use.

## Follow Windows

With **Follow Windows** on (the default), Nail uses your light theme while
Windows is in light mode and your dark theme while Windows is in dark mode
(Settings → Personalization → Colors → "Choose your mode" in Windows). Pick
the pair under the switch; the defaults are Frost and Graphite.

- The desktop app and `nail serve` on Windows read the Windows setting
  themselves and switch at once when you change it.
- When Windows uses a contrast theme (Settings → Accessibility → Contrast
  themes), Nail switches to **High contrast**.
- A browser connected through remote access follows its **own** light /
  dark mode (the phone's or the other computer's setting), not the mode of
  the computer Nail runs on; the desktop and other browsers keep theirs.
  Your theme choices (the pair, density, accent) are the same everywhere.
- A browser connected to a Nail that cannot read the Windows setting tells
  Nail its own light / dark mode instead.

Turning Follow Windows off keeps your pair, so turning it on again later
brings it back.

## Density

**Comfortable** (the default) leaves room around list rows and controls.
**Compact** fits more on the screen: smaller row padding, slightly smaller
text and controls, and the list rows drop their tag line (attachment,
invitation and label chips). Density applies to every theme.

## Text size, corner radius and accent colour

- **Text size** (11–20 px, default 14): the body text. Labels, headings and
  titles grow and shrink with it. In Compact, text is 1 px smaller.
- **Corner radius** (0–20 px, default 10): buttons and cards. Inputs, menus
  and windows follow in proportion; 0 gives square corners.
- **Accent colour** (`#rgb` or `#rrggbb`): buttons, links, the unread dot,
  focus rings, selections and pressed chips. It applies to the light and the
  dark theme alike.

Leave a field empty to use the theme's own value. Nail refuses values
outside the ranges and says why.

## Where the choice is kept

Everything on the Appearance page is written into `theme.kdl` in your
configuration folder, the same file a hand-written theme uses (see the
[theme reference](theme-reference.md)). Your comments and other lines in
that file stay as they are, and the previous version is kept in `backups/`.
For example:

```kdl
theme "My settings" extends="frost" {
    appearance follow-system=#true light="frost" dark="graphite" density="compact"
    tokens {
        color "accent" "#3b5bdb"
        font-size "body" 15
        radius "md" 8
    }
}
```

- `extends` is the theme used while Follow Windows is off.
- `appearance` holds Follow Windows (`follow-system`), the pair (`light`,
  `dark`) and `density` (`comfortable` or `compact`). Every property is
  optional.
- The three tokens are the text size, corner radius and accent overrides.
- Your own `part` rules in the same file apply on top of every theme.

Without a `theme.kdl`, or with `extends="default"` and no `appearance`
line, Nail follows Windows with Frost and Graphite. A `theme.kdl` without
`extends` is a complete theme of its own and is used as it is.

## Your own themes

Put theme files into the `themes` folder of your configuration folder, one
theme per file: `themes/<id>.kdl`. The file name is the theme's id: lower
case letters, digits and single hyphens, starting with a letter (for
example `themes/ocean-dark.kdl`). Nail lists your themes after the built-in
ones and picks up new, changed and removed files at once.

A theme file starts from the base theme or from another theme and changes
what it wants:

```kdl
theme "Ocean dark" extends="graphite" mode="dark" {
    tokens {
        color "accent" "#4fb3d9"
        color "canvas" "#0f1b24"
    }
}
```

- `extends` names `default` (the base), a built-in theme or another of your
  themes.
- `mode` (`light`, `dark` or `high-contrast`) decides where the theme is
  offered: light themes as the light partner of Follow Windows, dark themes
  as the dark partner. Without `mode`, the theme takes the kind of the theme
  it extends.

A theme that cannot be used is still listed with the reason, so you can fix
it: a mistake in the file (with the line and column), a parent that does not
exist or cannot be used, a theme that extends itself through others, or a
file named like a built-in theme (`frost.kdl` and so on — rename it). If the
theme you chose stops working, Nail uses Frost (light) or Graphite (dark)
until it is fixed.

The [theme reference](theme-reference.md) lists every token and part a
theme can change.
