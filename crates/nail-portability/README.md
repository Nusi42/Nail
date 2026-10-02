# nail-portability

Nail's importers, exporters, backup archives and diagnostics bundle (plan
M4c WP121; contract ADR 0094; design ADR 0096-portability). Pure
file work behind traits the core implements (`sink`, `backup`): readers
push messages and PIM items into sinks with per-item issues, writers pull
from sources; the core (`nail-core/src/portability.rs`) runs the jobs,
writes into the store / operation queues and reports progress.

| Module | What |
| --- | --- |
| `import` | previews and import jobs for every `ImportFormat` |
| `mbox` | streaming mbox reader (mboxrd / mboxo / mboxcl / mboxcl2) and writer |
| `eml`, `maildir` | `.eml` folders; Maildir (flags, Dovecot / Courier keywords, Maildir++) |
| `thunderbird` | profiles.ini, prefs.js accounts, local folders, SQLite / Mork address books, filters |
| `pim` | `.ics` / `.vcf` in (through `nail-ical`) and out |
| `export` | mbox / `.eml` / Maildir / `.ics` / `.vcf` export jobs |
| `backup` | `.nailbackup` (Argon2id + XChaCha20-Poly1305 STREAM over a zip), verify, restore plan, restore |
| `diagnostics` | redaction and the diagnostics bundle |
| `job`, `sink`, `error` | progress / cancellation / reports, the core's traits, errors |

Tests: `cargo test -p nail-portability` (unit tests per module,
`tests/thunderbird_profile.rs` over the committed synthetic profile in
`tests/fixtures/thunderbird`, `tests/catalog.rs` for the Fluent ids).
The feature `testing` exposes cheap Argon2id parameters for other crates'
tests.
