# ADR 0013: `accounts.kdl` — format, validation and format-preserving writes

Status: accepted · 2026-09-29 · M1a WP10 (`crates/nail-config/src/accounts/`)

## Context

`design.md` keeps account settings in a separate `accounts` config (not in
the theme or layout file) and credentials in the OS keychain, never in
files. Account setup happens mostly in the UI (wizard, settings, virtual
account editor), but users may edit the file by hand, share parts of it,
and keep comments in it. So the core must both read the file with precise
errors (like `theme.kdl` / `layout.kdl`, ADR 0006) and write it back without
destroying what the user wrote.

## Decisions

### File and model

- `<config dir>/accounts.kdl`, KDL v2, parsed by `nail_config::parse_accounts`
  into `nail_api::AccountsConfig` (ADR 0012). A missing or empty file is an
  empty configuration. Unlike theme and layout there is no `extends` and no
  shipped default.
- Top-level nodes, any order, all optional: `undo-send-delay <secs>`,
  `oauth-provider "<id>" …`, `account "<name>" …`,
  `virtual-account "<name>" …`. File order is display order.
- The account name is the account's `AccountId`: `[a-z0-9][a-z0-9._-]*`,
  at most 64 characters, unique among accounts and virtual accounts; `all`
  is reserved for the built-in "All accounts" virtual account. Names are
  stable because they also name keychain entries and appear in the store;
  a display name is separate (`name=`).
- Account: `account "<name>" email=… name=… enabled=… save-sent-copy=…`
  with children `imap` | `pop3` (exactly one), `smtp` (optional),
  `identity` (any), `sync` (optional).
- Servers: `host=` (required), `port=`, `tls="implicit"|"starttls"|"plaintext"`,
  protocol options as properties (`poll-interval`, `compress`,
  `max-connections`; `leave-on-server`, `delete-after-days`,
  `delete-when-deleted-locally`; `dsn`), and an optional `auth` child:
  `auth "password"|"oauth2"|"client-certificate"|"none"` with `username=`,
  `secret=`, `provider=` (OAuth2) and `mechanism=`.
- Identities: `identity "<address>" id=… name=… reply-to=… composer=…`
  with optional `bcc "…" …` and `signature "<text>" html=… markdown=…`.
  The identity id defaults to the address; ids are unique across the file
  (virtual accounts reference them).
- `sync bodies="full"|"headers-only"|"recent" days=… { folder path=…|role=…
  bodies=… days=… enabled=… idle=… }`; later rules override earlier ones
  field by field (`SyncPolicy::effective`).
- Virtual accounts: `members "<account>" …` (real accounts only),
  `merge-roles=`, `map "<folder>" { folder account=… path=… }`,
  `default-identity=`, `identity-rule "<pattern>" identity=…` with patterns
  `*@domain`, `*@*.domain` or a full address.
- OAuth2: `google` and `microsoft` are built in and need only
  `client-id` (and optionally a `client-secret` keychain reference); any
  other provider id needs `https://` `auth-url` and `token-url` (`http://`
  only for loopback hosts, for mock servers). `scopes "…" …` overrides the
  default scopes. This is where the maintainer's app registrations (plan, "Owner
  tasks") go.

### Defaults are filled in by the reader

To keep hand-written files short, the reader fills in what is left out:
the port from protocol and TLS mode (993/143, 995/110, 465/587), TLS
`implicit`, `auth "password"` with the account's address as user name, one
identity built from the account's address, the display name from the
account name. The API model is therefore always complete where clients
need values (host, port, TLS, auth, at least one identity), while purely
optional settings stay `null` (`ImapOptions`, `Pop3Options`, sync rules) so
that "not set" and "set to the default" remain distinguishable.

### Secrets

- Only keychain references are allowed: `secret="keychain:<service>/<name>"`
  (`nail_api::SecretRef`). Anything else is an error that points to the
  keychain.
- Properties that would hold a secret (`password`, `pass`, `token`,
  `access-token`, `refresh-token`, `private-key`) are refused with a
  specific message instead of "unknown property".
- A missing `secret` means the account's default reference
  `keychain:nail/<account>/<password|oauth2|client-certificate>`
  (`AuthConfig::secret_ref`). Incoming and outgoing servers therefore share
  a password by default, which matches most providers.

### Validation

Same machinery as ADR 0006: `ConfigError {kind, file, line, column,
message}` at the offending node or entry, first error only, nesting guard.
Beyond the shape it checks: unique names and identity ids, `all` reserved,
plausible email addresses, ranges (ports, `poll-interval` ≥ 30 s,
`max-connections` 1–16, `undo-send-delay` ≤ 600 s), mechanisms allowed for
the method and protocol, OAuth2 providers defined, virtual-account members
existing real accounts, mapped folders and referenced identities belonging
to members, contradictions (`days` without `bodies="recent"`,
`delete-after-days` with `leave-on-server=#false`), and folder rules that
change nothing.

### Writing: `AccountsDocument`

The UI edits accounts through requests; the core applies them to an
`AccountsDocument` (text + parsed model) and saves it atomically (temporary
file + rename).

1. The desired model is rendered in a canonical layout (the renderer).
2. The existing document is reconciled with it node by node (kdl's
   format-preserving document model):
   - top-level nodes are matched by kind and name; a node whose *meaning*
     is unchanged (compared on the parsed model) is not touched at all;
   - a changed node keeps its entries' formatting and position; only
     changed values are replaced, removed entries dropped, new ones
     appended; children are matched by name (singletons such as `imap`,
     `auth`, `sync`) or by name plus key (`identity "<address>" id=…`,
     `folder path=…|role=…`, `map "<name>"`) and reconciled recursively;
   - properties and children that only spell out a default (port, TLS, the
     default login, the implicit identity) are neither added nor removed,
     so editing one field of a terse hand-written account does not expand
     it;
   - removed nodes take the comment block directly above them along, but
     not text separated by a blank line (file headers survive);
   - new nodes go after their predecessor in the desired order, using the
     file's indentation unit and line endings (CRLF files stay CRLF).
3. The result is parsed again and must equal the desired model. If it ever
   does not (a writer bug), the canonical rendering of the whole file is
   used instead and a warning is logged; if even that does not round-trip,
   the edit is refused. An edit that produces an invalid file (unknown
   member, bad name, …) is refused with the reader's error, and the
   document is left unchanged.

`remove_account` also removes the account from virtual accounts (their
mappings, default identity and identity rules); a virtual account left
without members is removed. `derive_account_id` / `AccountsDocument::derive_id`
turn an address or name into a free id (`ada@work.example` →
`ada-work.example`, then `-2`, `-3`, …).

### Schema

`nail_config::accounts_schema()` (JSON Schema draft 2020-12, generated from
mirror types like the layout schema), snapshotted in
`crates/nail-config/schema/accounts.schema.json`, printed by
`nail config schema accounts`.

### Loading and watching

`ConfigManager` (theme / layout, hot reload) does not load `accounts.kdl`.
The core's account manager (WP14) owns it: it loads it at start-up, applies
UI edits through `AccountsDocument`, and should watch the file so hand
edits take effect (valid change → `Event::AccountsChanged`; invalid →
`Event::ConfigError` for `accounts.kdl`, keeping the last valid accounts).

## Alternatives considered

- **Accounts in the SQLite store only**: no hand editing, no sharing, no
  diffable backups; the design asks for a config file.
- **Rewriting the whole file from the model**: simple, but destroys
  comments and the user's layout on every UI change.
- **Text splicing by byte spans**: kdl's spans cover node names and entries
  but not always the trivia around them; editing the kdl document model and
  printing it keeps whitespace and comments reliably, and the round-trip
  check guards against surprises.

## Consequences

- Hand-edited and UI-edited files coexist; tests pin the preserving
  behaviour (`crates/nail-config/tests/accounts.rs`).
- New account settings are added in five places: the `nail-api` model,
  the reader, the renderer, the schema mirror and
  `userDocs/en/accounts-reference.md` (whose examples are parsed by a test).
