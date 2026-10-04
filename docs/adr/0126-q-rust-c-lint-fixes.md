# ADR 0126: lint fixes in the protocol, PIM, crypto and test-kit crates

- Status: accepted
- Date: 2026-10-04
- Wave: Q1 (quality baseline, ADR 0123), package q-rust-c

## Context

ADR 0123 turned on clippy pedantic, `unwrap_used` / `expect_used`, the new
rustc lints and the rustdoc lints, and made `#[allow]` an error. The crates
of this package (`nail-proto-imap`, `-pop3`, `-smtp`, `-gmail`, `-graph`,
`-ews`, `nail-portability`, `nail-ical`, `nail-exchange`, `nail-pgp`,
`nail-smime`, `nail-pkcs11`, `nail-testkit`, `nail-testkit-net`,
`nail-dav`, `nail-itip`, `nail-mime`, `nail-discovery`, `nail-auth`) had
974 clippy findings, 3 rustdoc findings, 16 `#[allow]`s, 3 markdownlint
findings, 19 typos findings and 8 possibly unused dependencies.

All 19 crates now pass `cargo clippy -p <crate> --all-targets --no-deps -- -D warnings`
(also with `--lib` alone and with `--all-features`) and
`RUSTDOCFLAGS="-D warnings" cargo doc -p <crate> --no-deps`. No lint level
in `Cargo.toml` / `clippy.toml` was changed.

## Decisions

### Mechanical fixes

- `cargo clippy --fix` for the machine-applicable findings
  (`unreachable_pub`, `redundant_closure_for_method_calls`, `trivial_casts`
  in FFI code, `borrow_as_ptr`, `map_unwrap_or`, ...), then `cargo fmt`.
- `let_underscore_drop`: `let _ = expr;` became `drop(expr);`, the fix the
  lint names, so the discarded value (mostly a best-effort I/O `Result`) is
  visibly thrown away. One case was a no-op binding and became a `_`
  pattern.
- `format_push_string` / `format_collect`: `write!` into the `String`
  (`use std::fmt::Write as _`), and a `fold` instead of collecting
  `format!`ed pieces. Writing to a `String` cannot fail, so production code
  ignores the `fmt::Result` with `let _ =`; test code unwraps.
- `assigning_clones`: `clone_from` / `clone_into`.
- Casts: `try_from` with an explicit fallback or error, or a type that needs
  no cast (`CHUNK_SIZE_U32`, `u32` curve sizes, `usize` lengths). DER
  length encoders (`nail-pkcs11`, `nail-smime`) were rewritten without
  truncating casts; the PKCS#11 one also encodes lengths of 64 KiB and more
  correctly now (new unit test).

### Panics and `expect`

- Constant parses moved out of public functions: the ISPDB base URL is a
  `LazyLock<Url>` (one `#[expect(clippy::expect_used)]` with a test that
  parses it); WebDAV extension methods come from one `dav_method` helper
  (same); the IMAP modified-UTF-7 alphabet is `base64::alphabet::IMAP_MUTF7`.
  The third `expect` is `nail-mime`'s `Parsed` wrapper, whose message is
  `Some` from construction until `Drop` takes it out.
- Invariants that the code can express were rewritten instead of
  `expect`ed: `let ... else` / `if let` chains (`nail-pgp` header folding,
  PGP/MIME signer, iCalendar END handling with `split_off`), a decrypting
  reader whose `load` returns the chunk it cached, a backup header encoder
  that returns its serialization error.
- Test doubles (`testing` modules of `nail-proto-ews`, `-graph`, `-gmail`,
  `nail-pkcs11`): a call that names a missing mailbox, folder or slot is a
  bug in the test, so it panics with a message that names the item
  (`unwrap_or_else(|| panic!(…))`) and the function documents it under
  `# Panics`.

### Integration tests

- `allow-unwrap-in-tests` covers `#[test]` functions and `#[cfg(test)]`
  items. Helper functions of an integration-test crate are neither, so every
  `tests/*.rs` file of these crates starts with `#![cfg(test)]`. Test
  targets are always compiled with `--cfg test`, so this changes nothing
  else; it states that the whole crate is test code, which it is, and keeps
  the helpers as strict as test functions (no exception attribute needed).
- The shared `tests/common/mod.rs` of `nail-proto-imap` and `nail-pgp` had a
  blanket `#![allow(dead_code)]` because every test binary used a different
  part of it. Each crate's integration tests are now one test binary
  (`tests/integration/main.rs` with one module per former file and a
  `common` module), so dead-code analysis sees every use and reports a
  helper nobody calls. It also links one binary instead of six / three.
- `nail-testkit` exports `NO_FLAGS` (an empty flag list) for
  `MailStore::append`, replacing the trivial cast `&[] as &[&str]`.

### `#[expect]` instead of `#[allow]`

Every remaining exception is an `#[expect(lint, reason = "…")]` on the
smallest item:

- `unsafe_code`: the Win32 code of `nail-discovery` (`DnsQuery_W`),
  `nail-exchange` (SSPI) and `nail-smime` (CryptoAPI / CNG). The
  module-level `#![allow(unsafe_code)]` became one `#[expect]` per function
  or `unsafe impl` that contains unsafe code; the `SAFETY` comments stay at
  each block. (`nail-pkcs11` has no unsafe code: `cryptoki` wraps the FFI.)
- `struct_excessive_bools`: configuration of the mock servers (independent
  feature switches), PKCS#11 token flags, `EwsOptions`, the mock IMAP
  session state.
- `too_many_arguments`: two restore steps of `nail-portability` and the
  generic labeled-list patch of `nail-ical` (one argument per aspect).
- `unnecessary_wraps`: the 22 operation handlers of the EWS mock share the
  dispatcher's `OpResult` signature.
- `struct_field_names`: `EccCmsSharedInfo` keeps the RFC 5753 ASN.1 names.
- `match_wildcard_for_single_variants`: `nail-pgp`'s algorithm names keep a
  wildcard because pgp's `draft-pqc` feature adds variants that feature
  unification may switch on.
- `cast_precision_loss`, `cast_possible_truncation` / `cast_sign_loss` and
  `reversed_empty_ranges` at three places whose reason is stated inline.
- The `#![allow(dead_code)]` on `nail-testkit-net`'s DAV XML module and the
  `cfg_attr(not(windows), allow(dead_code))` in `nail-discovery` are gone:
  the former was unneeded, the latter is `#[cfg(any(windows, test))]`.

### Other findings

- Debug: types holding secrets got a redacting `Debug` (`Secret` of the
  IMAP client, EWS `Password`, `DerivedKey` and the backup reader / writer);
  the others print their non-secret fields with `finish_non_exhaustive`.
- rustdoc: links to feature-gated `testing` items became plain code spans;
  `nail-ical`'s ambiguous `write` link points to `mod@write`; `nail-mime`
  links the public `sanitize_html` instead of a private module.
- Unused dependencies removed: `serde`, `serde_json` (`nail-auth`,
  `nail-proto-ews`), `nail-mime` (`nail-portability`), `ecdsa`
  (`nail-smime`; it uses `p256::ecdsa` / `p384::ecdsa`), `nail-api` and
  `serde` (`nail-testkit-net`).
- Markdown: code fences got a language.
- typos: real typos and accidental words fixed in code and test data
  (identifier names, a truncated SMTP keyword in a test, a French phrase,
  an opaque Graph change key). Legitimate data that remains needs entries
  in the shared `typos.toml` (see the package report): the German default
  folder names that `nail-proto-imap` recognizes, a two-letter Mork column
  id in a `nail-portability` test, and a folded name in
  `crates/nail-ical/tests/corpus/`.

## Consequences

- The crates are warning-free under the Q1 configuration; new code must
  stay so.
- Before ADR 0123's CI job is green, the crates these depend on
  (`nail-api`, `nail-store`, `nail-core`, owned by other packages) must be
  clean too: without `--no-deps`, `cargo clippy -p <crate>` also lints
  them.
- The non-Windows branches (`nail-discovery`, `nail-exchange` SSPI stubs)
  could not be compiled here (no Linux target installed); CI's Linux job
  covers them.
