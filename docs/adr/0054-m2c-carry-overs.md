# ADR 0054: M2d WP65 — M2c carry-overs: deterministic UI tests, dates in the UI locale, localized crypto / message-file texts, a11y sweep, snooze restarts

Status: accepted · 2026-09-30 · M2d WP65

## Context

Items left over from M2c (ADR 0049 and its verify run):

1. Vitest failed now and then while cargo built in parallel.
2. Dates in the UI must follow the UI locale (the Fluent locale in use),
   not the browser's.
3. OpenPGP key-import issues (`nail-pgp`) pointed at UI catalog ids in
   `security.ftl`, most `nail-pgp` errors reached the UI without a
   localized text, and the message-file import errors of `.eml` files were
   built in `nail-core` from English strings next to their ids.
4. The a11y Playwright scan listed windows and tabs by hand.
5. No core test reopened a Service with snoozes that are not yet due.

WP65 owns `crates/nail-pgp`, `crates/nail-mime`, `ui/src/i18n/`, the date
formatting code in `ui/src`, `locales/{en,de}/core-crypto.ftl`,
`e2e/tests/a11y-windows.spec.ts` and `crates/nail-core/tests/snooze_restart.rs`
(new files only in nail-core). Core wiring is requested from the
integrator (below).

## Decisions

### 1. Load-sensitive Vitest tests

Method: `npm test` (Vitest) ran 22 times while a clean
`cargo build -p nail-core --tests` looped in its own target dir (two
Vitest runs in parallel for 8 of them). Failures other than the ones of
the plug-in UI being written in parallel (WP62: `features/plugins/*`,
catalog coverage of its new ids, `source-rules` on `PluginTree.tsx`):

| Test | Runs failed | Cause |
| --- | --- | --- |
| `App.flows` › shows both accounts in the unified inbox | 3 / 8 (loop 1) | first full-app render of the file took 1.6–1.8 s; `findAllByRole` gives up after 1 s |
| `App.flows` › applies account status and account changes pushed by the core | 2 / 8 (parallel loops) | `findByRole("button", { name: "Office" })` after `AccountsChanged`, same 1 s budget |

Fix (no timeout raised): `App.flows.test.tsx` waits by **event-loop
turns**, not wall-clock time. `settleUntil(query)` and `eventually(assertion)`
run `act(() => setTimeout 0)` turns (at most 100) until the query finds
something / the assertion passes. The mock core answers within a turn and
nothing in these flows needs a real delay (measured: the message rows appear
after 1–3 turns), so a loaded machine only makes the waits slower. Every
`findBy*` / `waitFor` in the file is converted. The send countdown
("Sending in 10 seconds") read the wall clock with `/(10|9)/`; it now
accepts any second of the delay (transient-text race).

The helpers live in the test file (the harness is not WP65's); moving them
to `ui/src/test/harness.tsx` for other full-app suites is a follow-up.
Other suites did not fail in these runs.

### 2. Dates in the UI locale (`ui/src/i18n/format.ts`)

The call sites already passed `useLocale()` (the negotiated catalog
locale); the rule was not enforced and the formatting was copied into ten
files. Now:

- `formatDateTime(value, locale, options)` (cached `Intl.DateTimeFormat`;
  a string that is not a date is shown as is), `formatRelative`,
  `formatShortDate` (moved from `template/template.ts`, which re-exports
  them), presets `DATE_TIME`, `DATE_TIME_SECONDS`,
  `SHORT_DATE_TIME_SECONDS`, `FULL_DATE_TIME`, `DATE_ONLY`, and the hook
  `useDateFormat()` bound to `useLocale()`.
- Every date in `ui/src` goes through it (merge dialog, MCP audit, remote
  times, security dates, Sieve, junk status, snooze, reply / forward
  quotes, reader header, outbox, status bar, list templates).
- The UI locale is the localizer's: override (settings / `?lang=`), else
  the browser's languages negotiated against the shipped catalogs. A
  browser language Nail does not ship (French) formats English, the
  region of a matched language is not used (`de-CH` → `de`), a German UI
  in an English browser formats German.
- `format.test.tsx`: the hook against mocked `navigator.languages`, the
  outbox and message-list widgets under `locale="de"` in an `en-US`
  browser, and a source rule: no `new Intl.DateTimeFormat(`,
  `Intl.RelativeTimeFormat(` or `toLocale(Date|Time)String(` outside
  `i18n/format.ts`. `features/plugins/` (WP62, written in parallel) is
  excepted until the integration moves it onto the module.
- Native date inputs (`<input type="date">`, `datetime-local` in the
  snooze picker, "send later", rules, vacation) are rendered by the
  browser in its own language; replacing them is out of scope.

### 3. Localized texts of nail-pgp and nail-mime (`locales/en/core-crypto.ftl`)

- New catalog `core-crypto.ftl` (ids `pgp-*`, `message-file-*`), German
  twin empty. `detail` arguments are library texts (rPGP, base64, OS)
  and stay English.
- **Key import issues** (`KeyImportIssue.localized`): `pgp-import-unreadable`,
  `-no-key`, `-unusable`, `-keychain`, `-store` (were
  `security-import-issue-*` in the UI's `security.ftl`).
- **Errors** (`nail_pgp::Error::localized()`, attached by
  `From<Error> for ApiError`): `pgp-error-locked {key}`,
  `pgp-error-bad-passphrase` (the code's text `error-auth-failed` speaks
  of a server login), `pgp-error-crypto / -invalid / -unsupported {detail}`;
  new variant `Error::NotBase64` → `pgp-import-not-base64 {detail}`
  (was `Invalid("not base64: …")`). `NoKey`, store and keychain errors keep
  the code's text. Keychain errors now convert through `EngineError`, so
  their own `localized` survives.
- **Message files** (`nail_mime::message_file`): `read_message_file(path)`
  reads at most `MAX_MESSAGE_FILE` (50 MiB) + 1 bytes (no metadata race),
  parses and refuses empty / oversized / non-mail files as
  `MessageFileError` (`Unreadable {path, detail}` → `not_found`,
  `Size {path}`, `NotMail {path}` → `invalid_request`) with
  `message-file-unreadable / -size / -not-mail` and the previous English
  diagnostics; `From<MessageFileError> for ApiError`.
- Tests check every built id against `core-crypto.ftl` including the
  argument names.

### 4. a11y sweep (`e2e/tests/a11y-windows.spec.ts`)

One test per mode (default theme, `?mode=high-contrast`, `?mode=dark`)
against its own `nail serve --seed demo-multi` (plus a remote user so
"Pair a device" is enabled). It scans the main window, the reader, the
composer in all three modes, every layout window, every feature dialog
(`config-merge`, `config-errors`, `remote-pair`,
`security-autocrypt-import`, `snooze`, `sync-folder`), both context menus
and every settings tab, nested tabs included (found from the tab lists,
not a list in the spec). Violations are soft assertions, so one run
reports all. Coverage is asserted: the scanned windows must equal the
resolved layout's windows (`GetConfig`) plus `FEATURE_DIALOGS`, and the
scanned tabs the number of children of the settings window's `tabs`
nodes; a new window or tab fails until it is reachable and clean.
`a11y.spec.ts` stays as it is.

### 5. Snoozes across restarts (`crates/nail-core/tests/snooze_restart.rs`)

Complements `m2c_wired.rs` (wake time passed while stopped):
two pending snoozes (Inbox, explicit Archive wake folder) survive a
restart with the same wake times and folders, stay hidden (as does the
hidden folder), do not wake early, wake on time from the reloaded
scheduler (no request touches them), and the second survives another
restart; an unsnooze after a restart returns the mail at once, updates the
server, and nothing returns after a further restart.

## Consequences

- `nail-config`'s `every_catalog_file_is_embedded` fails until
  `core-crypto.ftl` is added to `EN_FILES` / `DE_FILES`
  (`crates/nail-config/src/l10n.rs`, integrator).
- Until the core forwards `nail_pgp::Error::localized()` and uses
  `nail_mime::read_message_file`, OpenPGP errors show the code's text and
  `.eml` errors keep `launch-error-*` (`core.ftl`); the requests are in
  the WP65 report.
- `security-import-issue-{unreadable,no-key,unusable,keychain,store}` in
  `security.ftl` are no longer produced; they go when `KeyTools.test.tsx`
  moves to the `pgp-import-*` ids.
