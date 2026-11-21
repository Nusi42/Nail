# 0225 — Values from the selected mail in the rules window

Status: accepted (2026-11-21)

## Context

ADR 0222 (R2 contract) left `valuesForCondition` and `ValuesFromMessage`
as stubs: when the rules window is opened from a mail, a condition's value
field should offer that mail's values. The maintainer also asked for the
same beside action fields where it makes sense (the mail's folder for
"move", its tags for "tag"), for the selected message when the window was
opened otherwise, and for "New rule from this message" / "Add to rule…" in
the message menus.

## Decision

- **Which message:** the rules store's `origin` (set by "Create rule from
  message" and "Add to rule…"); without one, the selected message
  (`selectedMessageId`). The button says which: "Values from this mail" or
  "Values from the selected mail" (`data-from="origin" | "selection"`).
- **Which values** (`valuesForCondition`, `valuesForAction`, pure
  functions over `MessageFacts` plus a `MessageValueContext` of the
  accounts, the folders of the mail's account and the mail's tags):
  - address conditions: address, name and domain of every address field
    (From, Sender, Reply-To, To, Cc; the condition's own field first, at
    most `MAX_ADDRESSES_PER_FIELD` addresses per field);
  - subject: the whole subject (`equals`) and its words (`contains`,
    `subjectWords`: at least `MIN_SUBJECT_WORD` characters, each once, at
    most `MAX_SUBJECT_WORDS`);
  - list-id: the `List-Id` (`contains`, as "Create rule from message"
    writes it);
  - header: the header's values where the facts hold them (From / To / Cc /
    Reply-To / Sender, Subject, List-Id; `contains`, since a header test
    sees the raw field), then every header name of the mail;
  - account, folder (by path, keeping the reference's account), tag;
  - actions: move / copy get the mail's folder, tag / untag its tags.
- **Picking** (`applyToCondition`, `applyToAction`) sets the value and the
  comparison that fits it — a domain becomes `ends-with "@<domain>"`, so a
  longer domain ending in the same letters does not match — and, for an
  address condition, switches the field to the value's (From → Cc) unless
  the condition's field covers it ("recipients" covers To and Cc). A header
  name becomes the header condition's name. The case setting is kept.
- **Component API:** `ValuesFromMessage { condition, onPick(value,
  condition) }` keeps the contract's `onPick(value)` shape and adds the
  whole condition as a second argument (an editor that sets only the text
  can ignore it). New beside it: `ActionValuesFromMessage { action,
  onPick(value, action) }`. Both render nothing without a message or
  without values for the field. The menu is the calendar's menu button
  (`features/pim/PimMenu.tsx`, WAI-ARIA menu button, shared
  `context-menu.*` parts) with a group header per source; parts
  `rules.from-message` and `rules.from-message.button`.
- **Tags** come from the mail's summary in the list cache (`findSummary`):
  the facts have none.
- **"Add to rule…"** (`rules.add-to-rule`, `./entryPoints.ts`): a menu
  source lists the saved rules (labels through `literalMenuLabel`); picking
  one (`rules.add-to-rule.pick`, argument: the rule id) opens the rules
  window on that rule with the mail as origin. Without the submenu the item
  opens the window with the mail as origin. The actions take the window
  name as an argument (`fromMessageActions(window)`), so the slot does not
  import the rules feature's barrel.

## Not done here (other packages' files)

- Placing the two controls in the condition and action rows (WR3).
- Registering `fromMessageActions(RULES_WINDOW)`, `fromMessageMenuSources`
  and `fromMessageActionInfo` in `features/rules/index.ts`, the menu items in
  `defaults/layout.kdl` (also "Create rule from message" in the reader card
  menu `context:thread-view.message`), and theme rules for
  `rules.from-message*`.
- Values of other headers (`X-Mailer`, …) and the mail's tags in
  `MessageFacts` need a `nail-api` change: proposed as
  `headers: Vec<(String, String)>` (capped) and `tags: Vec<String>`.

## Consequences

- One pure function per side (condition, action) decides the values;
  the component only renders them, so the rules editor stays free of
  message logic.
- Identical labels in different groups (the domain under From and To) are
  told apart by the group header; screen readers hear the label only.

## Integration

- The controls sit in every condition row and beside the folder of move /
  copy and the tag of tag / untag; the rows take the second `onPick`
  argument (the whole changed condition or action).
- "Add to rule…" is in the message-list row menu, the reader's menu and the
  reader toolbar's "turn into" group, and its submenu comes from the menu
  sources of ADR 0226. The reader card menu (`context:thread-view.message`)
  stays without the rules entries: its actions name the card, while these
  entries use the selected message.
- The button is the shared menu button (`pim.menu-button`) inside
  `rules.from-message`; there is no own button part.
