# ADR 0044 (M2c WP51): nail-sieve — Sieve compiler, round-trip harness, ManageSieve client, mock server

Status: accepted · 2026-09-30 · M2c WP51 (number to be assigned by the integrator)

## Context

Plan M2c WP51: compile the Sieve-expressible rules of `rules.kdl` (and the
vacation reply) into one Sieve script per account, say per rule why it is
`client` or `not-expressible`, upload it over ManageSieve (RFC 5804), and
test it all against a mock server. ADR 0043 fixed the contract
(`nail_api::sieve`: `ManageSieveConfig`, `RulePlacement`, `RuleSieveStatus`,
`SieveAccountStatus`, `VacationSettings`), the store (`sieve_state`,
`record_sieve_upload`) and the slots (`crates/nail-sieve`,
`crates/nail-testkit-net/src/managesieve.rs`, `locales/*/core-sieve.ftl`).
There is no Sieve library with an acceptable licence (`sieve-rs` is AGPL),
so compiler, parser and client are our own code.

## Decisions

### Crate shape

`nail-sieve` has no dependency on `nail-core`, `nail-store` or
`nail-rules`: it takes `nail_api` rule types and returns data.

- `compile(rules, vacation, &CompileOptions) -> CompiledScript` — pure.
  `CompileOptions` carries the account (and the virtual accounts that
  include it), the server's extensions (`COMMON_EXTENSIONS` before the
  first connection), a `FolderResolver` (mailbox names; `FolderMap` is a
  role table + hierarchy delimiter), the identities (vacation
  `:addresses`), the UTC offset (`:zone`), today (vacation dates without
  the `date` extension), `MAXREDIRECTS` and the previously active script.
  `CompiledScript` has the script, a `RuleSieveStatus` for every rule that
  applies to the account (in order), the required extensions, the
  `VacationOutcome` and the included script.
- `parse` (RFC 5228 section 8 grammar, hash comments kept on the next
  command) and `decompile` + `normalize_rule`: the **round-trip harness**.
- `SieveSession` (client), `SieveAuth` (IMAP login from keychain / OAuth
  provider, one token refresh), `upload` / `remove` (the managed script).
- `reasons` — every Fluent id the crate builds (`sieve-reason-*`,
  `sieve-error-*`), all in `locales/en/core-sieve.ftl` (a unit test checks
  both directions).
- New dependencies: `rustls-platform-verifier` (workspace, as IMAP/POP3)
  and `regex-syntax` 0.8 (MIT / Apache-2.0, already in the lock through
  `regex`). No new crates in `Cargo.lock`.

### Placement

A rule applies to an account when its scope is empty or has an entry for
the account, a virtual account containing it, or one of its folders. It is:

- **not-expressible** when any condition or action has no Sieve equivalent:
  `query`, `attachment`, `age`, `flag`, `tag` conditions (flags and tags do
  not exist before delivery), `folder` other than the Inbox, a regex outside
  POSIX ERE, an invalid regex / date, a header name with spaces or `:`;
  `forward`, `reply`, `notify` actions, moves / copies into another
  account, tags that are not IMAP atoms, redirects to non-addresses.
  Reasons carry `position` (condition path, `"1.2.1"`) or `action`
  (1-based).
- **client** when it could be compiled but: disabled, no `arrival`
  trigger, other automatic triggers (`on-send`, `schedule`, `older-than` —
  it would run in both places), scoped only to folders other than the
  Inbox, the server lacks a needed extension (`sieve-reason-extension`
  with the name), a target folder has no known mailbox name, or more
  redirects than `MAXREDIRECTS`.
- **server** otherwise. The core (WP55) demotes `server` to `client` with
  `demote_server_rules` (`sieve-reason-pending`, `-unavailable`,
  `-sieve-off`) until the script is actually uploaded.

### Semantics of the generated script

Nail's engine applies actions to one message in order and **the last move
wins**; Sieve files a copy per `fileinto`. So a script whose server rules
move, archive or delete requires `variables` and **defers delivery**: a
move sets `nail_deliver = "fileinto"` and `nail_folder`, a permanent
delete sets `nail_deliver = "discard"`, and one block at the end (and
before every `stop`) files or discards once. Tags and flags go to the
`imap4flags` internal variable, so they reach the final delivery wherever
they occur; `copy` is `fileinto :copy` and carries the flags set before
it; `redirect` is `redirect :copy` (Nail keeps the original). `archive` /
`delete` (not permanent) are moves to the Archive / Trash mailbox. Actions
after `stop` are dropped (the engine does the same).

Conditions: `all`/`any`/`not` → `allof`/`anyof`/`not` (`true` / `false`
for empty groups); `header` → `exists` / `header`; `subject` → `header
"subject"`; `body` → `body :text`; `size` → `size :over/:under`; `date` →
`date :zone :value "lt"/"gt" "date" "date"` (`date` + `relational`);
`auth` → `header :index 1 :contains "authentication-results" "dkim=pass"`
(`index`; Nail also reads only the topmost header); `account` and an Inbox
`folder` are constants for the account. Text ops: `equals` → `:is`
(trimmed), `contains` → `:contains`, `starts-with` / `ends-with` →
`:matches` with `*`, `?`, `\` escaped; `regex` → `:regex` with the pattern
rewritten from Rust syntax to POSIX ERE through the `regex-syntax` AST
(`\d` `\s` `\w` → `[0-9]` `[[:space:]]` `[[:alnum:]_]`, non-capturing /
named groups → groups; flags, lazy repetition, `\b`, Unicode classes,
class set operations, empty branches, `[:ascii:]` / `[:word:]` and
brackets containing `\`, `[` or a leading `]` are refused; the output is
valid Rust syntax and translates to itself). Case-insensitive is the
default comparator `i;ascii-casemap`; non-ASCII values use
`i;unicode-casemap` (extension `comparator-i;unicode-casemap`, else the
rule stays on the client; regexes with non-ASCII case-insensitive
patterns are not expressible because servers fold only ASCII in regexes);
case-sensitive is `i;octet`.

**Approximations** (documented for users in
`userDocs/en/server-side-rules.md`): an `address` condition becomes
`anyof(address :all …, header …)` — the address part is exact, the
display name is matched through the raw header (exact for `contains`,
approximate for `equals` / `starts-with` / `ends-with` of a name alone and
for anchored regexes); `list-id` equals / starts / ends also test the id
in angle brackets with `:matches "*<…>*"` like the engine; `.` in a POSIX
regex also matches a newline; the server only sees deliveries (a
`server` rule never runs for mail appearing in other folders).

With `variables` required, every literal string is written with
`${nail_dollar}{` for `${` (`set "nail_dollar" "$";` at the top) so user
text is never expanded — substitution is single-pass (RFC 5229), and no
extra extension (`encoded-character`) is needed.

### Vacation

`vacation :days N [:subject] [:from] [:addresses (settings + identities,
deduplicated)] text:…` placed **before** the rules (a `stop` or `discard`
must not skip it), wrapped in `if allof(…)` for `currentdate :value "ge" /
"le"` (with `date` + `relational`) and, with `skip_lists`, `not exists
"list-id"`, `not header :is "precedence" ["bulk","list","junk"]`, `not
header :matches "auto-submitted" "auto-*"`. Without `date`, Nail applies
the dates: `VacationOutcome` is `waiting` / `active` / `ended` and
`recompile_on` names the day the core must compile and upload again.
Without `vacation`: `unsupported`. Invalid or disabled settings: `off`.

### One managed script that preserves the user's scripts

The script starts with `# nail-managed-script`. `upload`:

1. lists the scripts; a script with Nail's name **without** the marker is
   the user's → `SieveError::NotManaged` (conflict), nothing written;
2. the previously active script is the active one that is not Nail's, or
   the one recorded in Nail's script (`# nail:previous "…"`) if it still
   exists;
3. compiles with the **server's** extensions and `MAXREDIRECTS`; the
   previous script is recorded and, with `include`, run first
   (`include :personal`, after the vacation block) so activating Nail's
   script does not switch the user's filters off;
4. `CHECKSCRIPT` (servers with `VERSION`), `PUTSCRIPT`, `SETACTIVE` (only
   when not already active); returns the compiled script, its SHA-256
   (same as `nail_store::hash_sieve_script`), extensions, other scripts,
   warnings.

`remove` re-activates the recorded script (or deactivates all when there
is none) and deletes Nail's; it also refuses a foreign script with Nail's
name.

### ManageSieve client

rustls with the ring provider and the platform verifier (or custom roots
in tests); `starttls` requires the server to offer `STARTTLS` and never
falls back; data buffered after the `STARTTLS` answer aborts (injection);
capabilities are re-read after STARTTLS (RFC 5804 2.2) and after the
login. A greeting that is not ManageSieve (IMAP / POP3 on the port) is
`NotManageSieve`; a refused connection (`ErrorKind::ConnectionRefused`)
and `NotManageSieve` are `is_unavailable()` → the core's `unavailable`
state. `AUTHENTICATE` sends the initial response (`PLAIN`, `XOAUTH2`,
`OAUTHBEARER`), answers an OAuth error challenge with `""`, cancels other
challenges with `"*"`, and refuses to log in without TLS unless the
account is `plaintext` (test servers); a server hiding SASL until
STARTTLS gives `EncryptionNeeded`. Scripts go as `{n+}` literals with
CRLF; server literals are capped at 16 MiB, lines at 64 KiB, every read
has the timeout. `NO` maps by response code (`QUOTA*` → `Quota` /
`too_large`, `TRYLATER`, `ENCRYPT-NEEDED`) and command (`PUTSCRIPT` /
`CHECKSCRIPT` complaints → `ScriptRejected` with the server's text as the
`detail` of `sieve-error-script`). `SieveError → EngineError` carries the
localized message.

### Mock ManageSieve server (`nail-testkit-net`)

`MockSieveServer` (plaintext / STARTTLS / implicit TLS; `PLAIN`, `XOAUTH2`,
`OAUTHBEARER` with or without initial response; configurable extensions,
`VERSION`, `MAXREDIRECTS`, script size and count quotas) implements every
command above plus `RENAMESCRIPT` and `NOOP`, records scripts, uploads and
checks, and injects faults (`fail`, `disconnect_on`,
`reject_scripts_containing`). `validate_script` checks scripts minimally
like a server: lexical syntax, balanced brackets, `;` / blocks, `require`
first and only for advertised extensions, known commands, extension
commands / tags / comparators only when required; errors name the line.

### Round-trip harness

`tests/roundtrip.rs` compiles a corpus that covers every expressible
condition, operator, comparator and action (including `${`, quotes,
backslashes, wildcards and non-ASCII text), then checks: all placements
are `server`; the mock's `validate_script` accepts the CRLF script;
`parse` + `check_requires` succeed; `decompile` gives back, per rule, the
original in canonical form (`normalize_rule`: account / Inbox conditions
evaluated, `Subject` / `List-Id` header aliases folded, `equals` trimmed,
regexes as ERE, folders as mailbox names, archive / trash as moves,
actions cut after `stop`, adjacent redirects merged) and the vacation
settings. Further tests pin the exact script text, placements and reasons,
capability downgrades and vacation date handling; `tests/client.rs` runs
the client and `upload` / `remove` against the mock.

## Consequences / for WP55

- Call `SieveAuth::from_config(account, imap.auth, …)` +
  `SieveConfig::from_endpoint(&sieve.endpoint(&imap))` →
  `SieveAuth::login` → `upload(session, sieve.effective_script_name(), rules,
  vacation, options)`; record with `Store::record_sieve_upload(account,
  &report.compiled.script, &report.extensions, &report.other_scripts, now)`;
  on error `record_sieve_error` with `EngineError::from(err)` (localized).
  `err.is_unavailable()` → state `unavailable`.
- `PreviewSieveScript` = `compile` with the last seen extensions (or
  `COMMON_EXTENSIONS`); `GetSieveStatus.rules` = `compiled.rules`,
  demoted with `demote_server_rules` unless the stored hash equals
  `script_hash(compiled.script)` compiled with the same inputs.
- Skip `server` rules on arrival; schedule a re-upload on
  `vacation.recompile_on`.
- demo-multi: start `MockSieveServer` next to the mock IMAP server.
- ManageSieve switched off → `remove`.
