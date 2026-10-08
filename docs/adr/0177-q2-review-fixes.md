# ADR 0177: Q2 review fixes — generated enum lists, the mock core checked, one home per helper

## Context

The two reviews of wave Q2 (SSOT / ODR / DRY, ADRs 0171–0176) found that the
duplicate check's "0 findings" did not cover several kinds of copies:

- contract limits copied by name into the UI's mock core (`MAX_EDIT_HISTORY`,
  `MAX_UI_STATE_KEYS`, `MAX_FREE_BUSY_DAYS`, …): the cross-language checks
  treated every `*mock*.ts` file as test code;
- contract enums whose Rust side has an `ALL` array or no list at all, typed out
  by hand in TypeScript (about 80 lists; some twice inside one feature), and
  Rust rules ported to TypeScript without vectors (`GmailLabelSettings::issues`,
  `AuthResults::summary`, `is_exchange_online_host`);
- multi-line constants (the `const-value` check compared single-line values
  only and cut strings at `//`);
- shared logic copied below jscpd's size: provider domain lists and the
  host-suffix matcher (8 copies, one without a label boundary), RFC 3339 / "now"
  helpers (about 25 copies), WCAG luminance (3 production copies with two
  thresholds), the skins' token maps (13) and visually-hidden rule (6), theme
  colours repeated as skin fallbacks, snapshot write-or-compare routines (6),
  test helpers (`must`, `mailEnv`, screenshot paths, window locators,
  `closeWindow`), `escapeHtml` (2), `ALL_ACCOUNTS`.

## Decisions

- **Enum lists are generated.** `ts_constants.rs` exports `<ENUM>_VALUES` for
  every contract enum the UI lists (`enum_values` with the enum's `ALL`, else
  `every!`, whose match fails to compile while a variant is missing; the
  order is the UI's), plus `MESSAGE_WARNING_KINDS` (from serialized samples
  under an exhaustive match), `RULE_SIMPLE_TRIGGERS` (`RuleTrigger::SIMPLE`),
  `GMAIL_SYSTEM_LABELS`, `GMAIL_CATEGORY_LABELS`, the mock core's limits,
  `MIN_PASSWORD_CHARS` (new `nail_api::remote` constant, the password policy's
  default), `MAX_QUERY_BYTES` / `MAX_QUERY_NESTING` (moved to
  `nail_api::search`; nail-search re-exports them), `ALL_ACCOUNTS`,
  `UPDATE_MANIFEST_FILE`, the junk keywords and the provider domains. New
  `ALL` arrays: `VaultPhase::ALL` (also the vault's phase order). Plug-ins get
  `PLUGIN_HOOK_KIND_VALUES`, `PLUGIN_COMMAND_SURFACE_VALUES` and
  `INVITATION_RESPONSE_VALUES` (the SDK's test host and manifest tool use
  them; `InvitationResponse` joins the plug-in bindings). Lists the UI owns
  (setup choices, sign-in methods, a mailbox's parts, tri-state choices, …)
  are defined once in the feature's model and imported by its messages.
- **The check sees them.** New SSOT checks: `generated-list` (a TS / JS list
  with exactly the values of a generated string-list constant: import it) and
  `ts-list` (one list of three or more strings defined twice in TS / JS).
  `const-value` compares whole values over any number of lines (comments
  dropped, strings kept intact, trailing commas ignored). The UI's mock core
  (`isMockCorePath`) counts as contract code for `rust-ts-const`, `ts-list`
  and `generated-list`; a TS constant that only names another constant
  (`= FOLDER_ROLES`, `= contract.UPDATE_MANIFEST_FILE`) derives its value and
  is no copy. Two coincidences are reasoned exceptions (CSS sides `left` /
  `right`; a demo card's key usages).
- **Rules ported to TS are pinned by vectors**: `labelIssues` (new `gmail`
  area), `authSummary` (`security`), `isExchangeOnlineHost` (`exchange`), and
  `parseHex` / `hexContrastRatio` (`appearance`).
- **One home per helper.**
  - `nail_api::hosts`: `GMAIL_DOMAINS`, `EXCHANGE_ONLINE_DOMAINS`,
    `OUTLOOK_COM_DOMAINS`, `is_under_domain` (label boundary),
    `is_gmail_domain`, `is_google_mail_host`, `is_exchange_online_host`,
    `is_microsoft_mail_host`. OAuth provider choice (nail-auth, Thunderbird
    import), "the server files the Sent copy" (outbox), Gmail discovery,
    Google DAV endpoints and Microsoft MX detection use them. Thunderbird's
    import no longer maps `notgmail.com` to Google (it matched any suffix);
    the Sent-copy rule now also knows `live.com`, `outlook.office.com` and
    Google's own hosts, the same providers OAuth knows.
  - `nail_util::time`: `unix_now` / `unix_now_secs` / `unix_now_millis` /
    `unix_now_nanos` / `unix_secs_of`, `rfc3339_utc` (+ `_opt`, `_millis`),
    `rfc3339_now` (+ `_millis`); every hand-written copy of these
    (chrono, `duration_since(UNIX_EPOCH)`) is gone, nail-core's own `unix_now` and
    nail-smime's `time` module with them; only clocks that hand out `DateTime`
    values (nail-core's `AdjustableClock`) still read chrono.
  - `nail_util::color` (hex, WCAG luminance with the sRGB threshold 0.04045,
    contrast ratio): nail-config's theme contrast test uses it; the UI's
    `theme/contrast.ts` is the one TS port (appearance's contrast warning,
    PIM colour tests, Exchange category text colour, skin tests).
  - `nail_util::snapshot` (`stored`, `check_text`, `UPDATE_SNAPSHOTS` /
    `UPDATE_GOLDEN` / `UPDATE_BINDINGS`): every snapshot, fixture, vector and
    golden file of the workspace.
  - UI: `theme/tokens.ts` (`T`, the one token map; fallbacks only for tokens
    the base theme does not define — none were left, so the dead
    `accent-soft` mix and the avatar / file colour fallbacks are gone),
    `declarations.ts` `VISUALLY_HIDDEN`, `theme/skin.ts` `avatarPaletteRules`
    over `ACCOUNT_COLOR_SLOTS` and `widgets/fileKinds.ts` `FILE_KINDS` (theme
    colours come only from `defaults/theme.kdl`), `text/html.ts` `escapeHtml`,
    `test/mailEnv.ts`, `test/must.ts` everywhere.
  - e2e: `support/nail.ts` `screenshotFile`, `support/views.ts`
    `windowSelector` / `windowById` / `closeWindow({ ifOpen, dispatch })`;
    scripts: `lib/work-folder.mjs` `HOOKS_PATH_ARGS`, generated constants for
    update channels, the manifest name and the visual review's theme and
    density order.
- **SMTP honours every `TlsTrust`**: `WebPkiRoots` maps to lettre's
  `CertificateStore::WebpkiRoots` (lettre feature `webpki-roots`, the crate
  nail-net already uses); the match is exhaustive.
- `UNSUPPORTED_CODES` is a `ReadonlySet<ErrorCode>`.
- **Found by the verify run:** a seeded desktop run on given directories
  (`--seed demo --data-dir …`, `desktop-m4b.spec.ts` "nail plugin dev") used
  WebView2's default profile, which an installed Nail that is running holds
  (WebView2 `0x8007139F`). Its windows (main, pop-outs, print) now share
  `<data-dir>/webview` (`webview_dir` in the shell), one value for all three.

## Consequences

- A new contract enum the UI lists needs one `enum_values` line; a hand list of
  its values fails `npm run check`.
- The mock core can no longer drift from the core's limits silently.
- Behaviour is unchanged except the two host fixes above and the mock search
  limit, which now counts UTF-8 bytes like the core.
