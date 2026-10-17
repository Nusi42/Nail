# ADR 0182: only the user's data is selectable

Status: accepted · 2026-10-08 · maintainer finding (desktop test of 4290474)

## Context

Requirement: only text that is not one of the app's own labels may be selectable. Every label, title, button
caption and list row of the app could be selected, and a drag across the list painted a selection over rows.

## Decisions

- One list, `SELECTABLE_PARTS` in `ui/src/theme/selection.ts`, names the parts whose text is the user's data:
  the reader's subject, header values, participants, plain-text bodies and quotes; invitation details and
  sender checks; keys, fingerprints, certificates, the recovery key, pairing codes, the MCP token, remote URLs;
  contact details; error details, configuration errors, diagnostics, import issues, plug-in logs, the MCP
  audit, merge differences, the Sieve script; plug-in text and Markdown. Text fields, text areas and editors
  (`contenteditable`) are always selectable.
- `SELECTION_CSS` = `body { user-select: none }` plus `user-select: text` for that list. The compiler emits it
  right after the structural base CSS, so every skin and theme rule still wins (`mcp-token.value` keeps the
  theme's `user-select: all`); no inline styles, no per-component rules.
- Mail documents in their frames are documents of their own and stay selectable.
- List rows stay chrome (they are clicked, dragged and picked, not read from): their preview is not
  selectable, and dragging across the list picks rows, never text. A reader card's header is a button (its
  sender name is not selectable); the conversation's subject and participants above the cards are.
- The same list decides the text menu (ADR 0184): `selectableRoot` finds the selectable element around a
  right-click.

## Consequences

A new part with user data joins `SELECTABLE_PARTS` (a unit test checks that every listed part is rendered).
The reader shows addresses only where the layout shows header fields (a message alone, or `headers=` on the
`thread-view`); the cards' headers show names.
