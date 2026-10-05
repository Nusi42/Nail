# ADR 0161: The accent override is ignored in high-contrast themes

Status: accepted · 2026-10-05 · maintainer decision (amends ADRs 0102, 0104, 0107, 0135, 0138)

## Context

The Appearance page's accent colour is an ordinary token of the user's
`theme.kdl` (`color "accent"`, ADR 0102) and, like the text size and the
corner radius, it was laid over every theme — High contrast included. High
contrast is built so that every text and outline reaches 7:1 (its accent is
pure yellow on black, with `accent-fg` black on it); an accent picked for
Frost or Graphite (say `#3b5bdb`) breaks that contrast on the black
surfaces, and it did so silently whenever Follow Windows switched to High
contrast because a Windows contrast theme came on. Decision of the
maintainer: ignore the accent override in high-contrast themes.

## Decision

- **Which themes.** The accent override is left out whenever the *active*
  theme's kind is `high-contrast`: High contrast picked as the fixed theme,
  High contrast chosen by Follow Windows (also per remote session, which
  resolves through the same path), and any user theme whose kind is high
  contrast — the kind of a user theme is the `mode` of its resolved
  `extends` chain, so a theme extending High contrast counts unless it sets
  another `mode`, and a theme of its own with `mode="high-contrast"` counts
  too. One rule, `nail_api::appearance::accent_ignored(active, themes)`,
  mirrored by the UI's `accentIgnored` (`ui/src/features/appearance/model.ts`).
- **Where.** `nail_config::resolve_selected` — the only place the rendered
  theme is built (host, `ConfigManager::appearance_for` for remote sessions,
  edit validation) — drops the *base value* of the user's `color "accent"`
  from a sparse `theme.kdl` before merging it onto the active theme, so the
  theme's own accent and its per-mode values (e.g. `print=`) apply. Mode
  values the user wrote by hand on that token stay. A complete `theme.kdl`
  (no `extends`, pre-M5) is used as it is.
- **Stored value.** Nothing is written: `theme.kdl` keeps the accent,
  `AppearanceSettings.overrides.accent` still reports it, and it applies
  again as soon as another theme is active. Text size and corner radius keep
  applying in every theme.
- **No new API field.** The UI already has `Appearance.active` and every
  theme's `kind`, so it derives the state itself: Settings → Appearance shows
  the note "High contrast uses its own colours." (`appearance.accent-note`,
  `appearance-accent-high-contrast`) and disables the accent field and the
  colour picker (`aria-describedby` the note); the stored value stays
  visible. The theme editor does not mark a high-contrast theme's accent
  "Overridden in Appearance" and previews / checks contrast with the theme's
  own accent. The mock core resolves the same way.

## Consequences

- High contrast keeps its AAA contrast whatever accent the user picked for
  the other themes; switching Windows' contrast theme on and off no longer
  needs any change in Nail.
- A user who wants a different accent in a high-contrast theme writes it
  into that theme's file (a user theme extending High contrast, edited in
  the theme editor), where the contrast check sees it.
- Tests: `nail-api` (`accent_ignored`), `nail-config` (`themes.rs` unit test,
  `tests/appearance.rs`: fixed / Follow Windows / user theme, stored value
  kept, applies again, remote session), Vitest (`appearance.test.tsx`,
  `themeEditor.test.tsx`), e2e `m5a.spec.ts` ("High contrast keeps its own
  accent").
