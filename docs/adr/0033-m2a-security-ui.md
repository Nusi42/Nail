# ADR 0033 (M2a WP34): Security UI

Status: accepted · 2026-09-29 · M2a WP34

## Context

WP34 builds the security UI of wave M2a (plan section 7) against the
contract of ADR 0029: the key and certificate manager, per-identity crypto
settings, composer sign / encrypt toggles with live recipient key status,
the message security badge with a details popover, the sender
authentication badge, link and attachment warnings in the reader, the
encryption-at-rest settings with progress, and the Autocrypt setup
message flow. It runs in parallel with WP31–WP33 and may only edit
`ui/src/features/security/` and `locales/{en,de}/security.ftl`. The
composer (WP21's `features/composer/`), the reader
(`widgets/MessageView.tsx`), the account settings, the layout, the theme
and `nail-config` belong to others, so everything outside the slot is a
request to the integrator (WP35), listed below.

## Decisions

### Widgets first, components for mounting

Every piece is a **layout widget**, so the layout places it and nothing
else has to change for it to work: the design says the sender badge's
place is the layout's decision, and the same holds for the rest.

| Widget kind | Component | What |
| --- | --- | --- |
| `security-keys` | `KeyManager` | keyring list with filters, details, trust editor, export, unlock, delete; tools: import (paste, key files, PKCS#12), create key, find online, Windows store |
| `security-identities` | `IdentityCryptoSettings` | crypto settings of every identity, saved per account with `UpdateAccount` |
| `security-settings` | `SecuritySettingsWidget` | encryption at rest (state, progress, "Recommended: turn this on", confirm, retry), key lookup policy and sources, decrypted-indexing opt-in per account |
| `security-autocrypt` | `AutocryptSetup` | send a setup message (code shown once), import the selected setup message with its code |
| `security-message` | `SecurityMessageWidget` | security badge + sender badge of the selected message |
| `security-warnings` | `SecurityWarningsWidget` | warnings banner of the selected message |
| `security-composer` | `ComposerSecurityWidget` | toggles and recipient key status for the open composer (`uiStore.composer`) |

None of the widgets takes options. The same pieces are exported as
**controlled components** for packages that prefer to mount them inside
their own UI (see "Mount points").

Action `security.autocrypt.import` opens the dialog
`security-autocrypt-import` (enabled while a message is selected).

### Composer: explicit choice, suggestion, guard

`composerStore.ts` (Zustand, outside `uiStore`) holds the composer's
choice. `choice === null` means "not touched": the toggles show the core's
`RecipientKeysReport.suggested` (identity defaults, Autocrypt `mutual`).
Once the user flips a toggle the explicit `SendCrypto` is kept; "Use
defaults" goes back to `null`. `composerCrypto()` returns what belongs in
`Draft.crypto` / `SendDraft.crypto` (`undefined` = identity defaults).

`CheckRecipientKeys` runs debounced (300 ms) on the distinct, lower-cased
recipients of To / Cc / Bcc. `lookup` is `true` only when the security
settings allow it: automatically for `automatic`, after the user clicks
"Look up N missing keys online" for `on-demand`, never for `off`. A key
found online is not imported silently: "Use key found online" does
`LookupKeys` + `ImportKeys` on the user's click.

The **send guard** (`model.ts sendBlock`) mirrors the core's rule that a
chosen protection never falls back to plain text: it names the recipients
without a usable key, a missing own key per technology, or a missing
recipient; the composer shows it as an alert and `composerSendBlock()`
lets the composer disable sending. The technology resolves as the core
does: explicit choice → identity preference → the first technology that
works for everyone (OpenPGP before S/MIME) → OpenPGP.

### Badges

`securityVerdict` turns a `SecurityStatus` into a signature verdict
(`verified`, `valid` = signed but not verified, `mismatch`, `unknown-key`,
`key-problem`, `invalid`, `error`), an encryption verdict and a tone
(`good` / `neutral` / `warning` / `danger`, `data-tone`). The worst
signature wins. Only keys at `autocrypt` or better (or an S/MIME chain
state `valid`) count as verified; `autocrypt-gossip` does not. A
`partial` message or a header-protection `mismatch` is never shown as
good. The popover shows signer, fingerprint (groups of four), trust,
signing time, the S/MIME chain with "revocation was not checked", the
encryption state, header protection, and offers "Mark key as verified"
(`SetKeyTrust`) for unverified OpenPGP keys and a passphrase prompt
(`UnlockKey`, keep unlocked for 0 / 15 / 60 minutes or the session) when
the key is locked. The sender badge uses the same summary as the core's
`AuthResults::summary` and lists DMARC, DKIM, SPF and ARC with domain,
selector, alignment and reason.

`Popover` is a small in-feature component: a badge button with
`aria-expanded`, a `role="dialog"` panel as `<part>.popover`, closed by
Escape (focus returns to the badge) and by a click outside. Placement is
left to the theme.

### Warnings

The reader shows one banner listing every link warning with its **real
target** and every risky attachment. The mail HTML is in a sandboxed
iframe, so the UI cannot decorate the links in place; `warningsForLink`
is exported for a later hover or click-through confirmation in the
reader, and `AttachmentWarningMark` for the attachment list.

### Mock

`mock.ts` now implements every security request (the WP30 mock answered
`unsupported` for most). Key material is fake: exports are armored / PEM
blocks whose `Comment:` headers carry fingerprint and user ids, and
imports read them back, so export → import, lookup → import and PKCS#12
(password `demo`) work end to end in tests and in `?mock=1`. Trust
decisions are kept per (technology, fingerprint) across delete and
re-import, as in store v5. The vault migration steps through the three
phases with `VaultProgress` events and can be made to stop with an error
(`vaultError`). `decorateMessageBody` adds per-message or per-sender
findings to `GetMessage` answers; `securityDemoBySender()` has demo
findings for the mock mailbox's senders (every badge and warning kind).
Sending with sign / encrypt stays refused (`api/mockOutbox.ts`, ADR 0029).

### Strings

All strings are in `locales/en/security.ftl` (368 messages;
`locales/de/security.ftl` stays empty). Ids built at runtime are listed in
`messages.ts`, including the layout labels proposed below
(`security-settings-tab-*`, `security-action-autocrypt-import`) and the
dialog title `window-security-autocrypt-import`.

## Mount points (for the integrator)

- **Composer** (`features/composer/`):
  - Place the widget `security-composer` in the composer window (layout),
    or mount `<ComposerSecurity identityId recipients choice onChoice
    onReport />` inside `Composer.tsx`.
  - `flows.ts` / `draft.ts`: put `crypto: composerCrypto()` into the
    `Draft` built for `SaveDraft` (it is `undefined` when untouched) and
    into `SendDraft.crypto`; when a draft is opened, call
    `loadComposerCrypto(draft.crypto)`; add `crypto` to `draftKey` so a
    toggle triggers autosave.
  - Before sending, `composerSendBlock()`: non-`null` → do not send, show
    the block (its `id` is a Fluent id with `count` / `recipients`).
- **Reader** (`widgets/MessageView.tsx` or the layout):
  - Layout: `security-message` (badges) next to the header and
    `security-warnings` above the body in both reader layouts; or mount
    `<MessageBadges message />` and `<MessageWarnings message />` inside
    `MessageView`.
  - In the attachment list, next to each file name:
    `<AttachmentWarningMark warnings={message.warnings} index={a.index} />`.
- **Account settings** (`widgets/AccountSettings.tsx`): optionally mount
  `<IdentityCryptoFields identity ownKeys onChange />` in each identity
  (own keys from `ListKeys {own_only: true}`); otherwise the separate
  `security-identities` tab does the job. Note: AccountSettings saves the
  identities it loaded, so it must keep `crypto` when it saves (it does
  as long as it spreads the loaded identity objects, as today).
- **Message list / actions**: add `security.autocrypt.import` to the
  reader's toolbar or menu (`label="@security.action.autocrypt-import"`).

## Requests to the integrator

1. `crates/nail-config/src/widgets/security.rs`: register the kinds
   (no options):

   ```rust
   pub(crate) const WIDGETS: &[WidgetSpec] = &[
       widget("security-keys", &[], &[], false),
       widget("security-identities", &[], &[], false),
       widget("security-settings", &[], &[], false),
       widget("security-autocrypt", &[], &[], false),
       widget("security-message", &[], &[], false),
       widget("security-warnings", &[], &[], false),
       widget("security-composer", &[], &[], false),
   ];
   ```

   plus a parse test like `mcp.rs`, and `userDocs/en/layout-reference.md`.
2. `ui/src/api/mock.ts` `GetMessage`: answer
   `decorateMessageBody(this, body)` (from `features/security/mock`), and
   for the dev mock (`?mock=1`) set
   `data.features.security.bySender = securityDemoBySender()`.
3. `defaults/layout.kdl`: settings tabs

   ```kdl
   security-keys id="security-keys" label="@security.settings.tab.keys"
   security-identities id="security-identities" label="@security.settings.tab.identities"
   security-settings id="security-settings" label="@security.settings.tab.protection"
   security-autocrypt id="security-autocrypt" label="@security.settings.tab.autocrypt"
   ```

   `security-message` and `security-warnings` in both reader panes (above
   `message-view`), `security-composer` in `window "composer"`, and the
   action `action "security.autocrypt.import" label="@security.action.autocrypt-import"`.
   Then regenerate the nail-api fixtures.
4. `defaults/theme.kdl`: style the parts (appendix). New shared part:
   `form.file` (the file input of the import tool).
5. Composer and reader wiring as in "Mount points"; a Playwright scenario
   per WP35's list uses these widgets.

## Consequences

- The UI is complete against the mock; with WP35 wiring the core it needs
  no change except where the core's answers differ from the mock's.
- 115 new Vitest tests (model, mock, every widget and flow); the whole
  suite: 534 tests green, lint clean.
- No new dependencies.

## Appendix: theme parts (proposal for `defaults/theme.kdl`)

```kdl
    // ---- Security (ADR 0033) ----
    part "security-message" { display "flex"; flex-wrap "wrap"; gap "$space.xs"; }
    part "security-badge.anchor" { position "relative"; display "inline-block"; }
    part "auth-badge.anchor" { position "relative"; display "inline-block"; }
    part "security-badge" {
        display "inline-flex"; align-items "center"; gap "$space.xxs"
        padding "$space.xxs" "$space.sm"; border "$border.thin"
        border-radius "$radius.sm"; font-size "$font-size.sm"
        color "inherit"; background-color "$color.bg-subtle"
    }
    part "security-badge[tone=\"good\"]" { color "$color.success"; }
    part "security-badge[tone=\"warning\"]" { color "$color.warning"; }
    part "security-badge[tone=\"danger\"]" { color "$color.danger"; }
    // auth-badge: same rules as security-badge.
    part "security-badge.popover" {
        position "absolute"; z-index 20; min-width 320; max-width 480
        padding "$space.md"; border "$border.thin"; border-radius "$radius.sm"
        background-color "$color.bg"; box-shadow "$shadow.overlay"
    }
    // auth-badge.popover: same rules.
    part "security-details.row" { display "grid"; grid-template-columns "10em 1fr"; gap "$space.sm"; }
    part "security-details.warning" { color "$color.warning"; }
    part "security-fingerprint" { font-family "$font.mono"; }
    part "security-trust[trust=\"rejected\"]" { color "$color.danger"; }
    part "security-trust[trust=\"verified\"]" { color "$color.success"; }
    part "security-trust[trust=\"own\"]" { color "$color.success"; }
    part "security-chain.revocation" { color "$color.fg-muted"; font-size "$font-size.sm"; }
    part "security-warnings" {
        padding "$space.sm" "$space.md"; border-left "3px solid $color.warning"
        background-color "$color.bg-subtle"
    }
    part "security-warnings.target" { display "block"; font-family "$font.mono"; font-size "$font-size.sm"; word-break "break-all"; }
    part "security-attachment-warning" { color "$color.danger"; font-size "$font-size.sm"; }
    part "security-compose" { display "flex"; flex-direction "column"; gap "$space.xs"; }
    part "security-compose.toggles" { display "flex"; flex-wrap "wrap"; align-items "flex-end"; gap "$space.sm"; }
    part "security-compose.recipient" { display "flex"; flex-wrap "wrap"; gap "$space.sm"; font-size "$font-size.sm"; }
    part "security-compose.recipient[state=\"usable\"]" { color "$color.success"; }
    part "security-compose.recipient.state" { color "$color.fg-muted"; }
    part "security-compose.block" { color "$color.danger"; }
    part "security-keys", "security-identities", "security-settings", "security-autocrypt" {
        display "flex"; flex-direction "column"; gap "$space.md"; padding "$space.md"; overflow-y "auto"
    }
    part "security-keys.tools", "security-keys.filters" { display "flex"; flex-wrap "wrap"; gap "$space.sm"; align-items "flex-end"; }
    part "security-keys.list" { display "flex"; flex-direction "column"; gap "$space.xxs"; margin 0; padding 0; }
    part "security-keys.key" {
        display "flex"; flex-wrap "wrap"; align-items "center"; gap "$space.sm"; width "100%"
        padding "$space.xs" "$space.sm"; border "none"; border-radius "$radius.sm"
        text-align "left"; color "inherit"; background-color "transparent"
    }
    part "security-keys.key:hover" { background-color "$color.bg-subtle"; }
    part "security-keys.key:selected" { color "$color.selection-fg"; background-color "$color.selection"; }
    part "security-keys.key[key-state=\"revoked\"]", "security-keys.key[key-state=\"expired\"]" { opacity "$opacity.muted"; }
    part "security-key.state" { color "$color.warning"; }
    part "security-key.exported.text" { font-family "$font.mono"; min-height 160; }
    part "security-vault.recommendation" { color "$color.accent"; font-weight "$font-weight.medium"; }
    part "security-vault.bar" { width "100%"; }
    part "security-vault.failed", "security-settings.warning" { color "$color.warning"; }
    part "security-autocrypt.code.value" { font-family "$font.mono"; font-size "$font-size.lg"; }
    part "security-autocrypt.code.warning" { color "$color.warning"; }
    part "form.file" { font "inherit"; }
```

(The token names follow the existing theme; adjust where a token such as
`$font.mono` is named differently. Several selectors per `part` are
shorthand here — write one `part` per selector if the theme format needs
it.)
