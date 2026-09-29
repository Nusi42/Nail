# ADR 0035: M2a review fixes: Autocrypt address binding, signer binding, MCP encrypted gate, encrypted drafts, authserv-id

Status: accepted · 2026-09-29 · M2a review

## Context

The M2a security review found that an Autocrypt key could vouch for any
address written into it, that the signer of a signed part was compared with
a `From` the reader does not show, that the MCP `encrypted` grant was
enforced with a byte heuristic that missed mail the reader decrypts, that
drafts of mail that goes out encrypted still reached the server's Drafts
folder in plain text, and that a sender could poison the learned
`authserv-id`. This ADR records the fixes. ADRs 0030, 0032 and 0034 point
here where a fix changes what they say.

## Decisions

### An Autocrypt key vouches only for its header's address (`nail-pgp`)

- `key::bind_to_address(key, addr)` keeps only the user ids for `addr`
  that carry a valid self-certification (with all their self-signatures,
  so revocations stay) and drops every other user id and user attribute;
  it fails when no such user id exists. `AutocryptHeader::bound_key` uses
  it; `process_incoming` stores sender and gossip keys only through it. A
  header whose key has no user id for `addr` is invalid (Autocrypt Level 1
  §2.1: the key must be usable for `addr`), and the peer state is updated
  as for a message without a header.
- Autocrypt trust is **per (key, address)**: `Keyring::trust_for(key,
  email)` gives a key whose source is `autocrypt` (`autocrypt-gossip`)
  that level only for an address whose peer record — in any account —
  names its fingerprint as `key_fingerprint` (`gossip_fingerprint`);
  for every other address it is `unknown`. User decisions and own keys
  apply to all addresses. `Store::autocrypt_peers_for_email` reads the
  peer records of an address across accounts.
- `recipient_key(email)` ranks candidates by `trust_for(key, email)`, and
  signature verification reports `trust_for(key, signer)`. A key the user
  imported with extra addresses and whose source Autocrypt later raised is
  therefore not trusted for those extra addresses, and cannot outrank a
  genuine key for them by trust. The key list (`to_crypto_key`) still
  shows the key's source-level trust.

### The signer is bound to the displayed `From` (`nail-pgp::read`)

`displayed_from(outer, protected)` is the `From` the reader shows: the
protected `From` when the signed / decrypted part carries a header
protection marker (`protected-headers` / `hp`; the display then replaces
the outer header, also for a `Mismatch`), else the outer `From`. Only its
first address counts, as for S/MIME. `Verifier::bind` sets `signer`,
`signer_matches_from` and the per-address trust against it; it runs for
PGP/MIME signed, encrypted, nested sign-then-encrypt (the outer
signatures are re-bound to the display's `From`), the mailing-list
`multipart/mixed` case (the raw message is shown: outer `From`) and inline
PGP. A `From` inside the signed part without a marker is never used.

### The MCP `encrypted` gate is the reader's result (`nail-mcp`, `nail-core`)

- `CoreAccess` gains `is_encrypted(id)`; `CoreData::is_encrypted`
  forwards to it, and `nail_mcp::core_data::is_encrypted_mime` is gone.
- The core answers with `Service::message_is_encrypted`: the cached
  `message_security` status says encrypted, or the cheap
  `crypto::is_encrypted_raw` says so, or else `open_message` (the code
  `GetMessage` runs) reports encryption. Both shortcuts can only add
  gating. So inline PGP in base64 / quoted-printable or mid-line, S/MIME
  relabelled as `application/octet-stream; name=*.p7m`, and enveloped
  data inside a signed layer are gated like the reader decrypts them —
  for `readable_message` and for the `hide_encrypted` search filter.
  Messages whose body is not downloaded are not encrypted (no content).
- `is_encrypted_raw` (the byte heuristic) moved into `nail-core` and only
  picks messages for decrypted indexing and rules. Everything it selects
  for decrypted indexing is gated by the MCP check.

### Drafts of encrypted mail stay off the server (`nail-core::composer`)

`upload_draft_copy` decides with `draft_goes_encrypted`: the draft's
explicit choice, else the identity defaults (`effective_crypto`, as
`SendDraft`), else an Autocrypt recommendation to encrypt for the draft's
recipients (the composer's suggestion; the UI saves only an explicit
choice in `Draft.crypto`). When the recommendation cannot be computed the
draft stays local. When a draft goes encrypted, a server copy uploaded
earlier (before encryption was switched on) is deleted through the
operation queue and the draft's pointer to it cleared. An explicit
"no encryption" choice still uploads. The UI is unchanged.

### `authserv-id` is trusted only when configured (`nail-core`, `nail-mime`, UI)

- `trusted_authserv_ids` returns only `AccountConfig::authserv_ids`. The
  core no longer learns and stores an id (`app_settings`
  `authserv-id/<account>` values written by earlier builds are ignored):
  when the receiving server adds no `Authentication-Results`, any id that
  most recent mail carries can come from senders.
- New request `SuggestAuthservId { account_id }` →
  `AuthservIdSuggestion { account_id, authserv_id? }` (unknown account:
  `not_found`), computed on request from the 20 newest inbox messages
  and never stored. `learn_authserv_id` counts a message only for an id
  whose organizational domain matches the `by` host of its newest
  `Received` header that names a DNS host (the account's own mail system;
  address literals such as Gmail's internal `by 2002:…` hop are skipped),
  so ids that senders make up are not proposed. A sender can still name
  the receiving server's own domain; the user confirms by saving.
- Account settings get a **Sender checks** section: the trusted servers
  (`authserv-ids`, space or comma separated) and **Suggest from recent
  mail**, which fills in the suggestion and says to compare it with a
  message's source; nothing is trusted before **Save**.

## Consequences

- Accounts without a configured `authserv-id` show no sender badge until
  the user saves one (the demo accounts configure theirs).
- Autocrypt keys arriving with extra user ids are stored without them;
  merging with an imported copy of the same key keeps the imported user
  ids, but trust for them stays `unknown`.
- Tests: `nail-pgp` (`key` unit test, `tests/autocrypt.rs`
  `autocrypt_keys_vouch_only_for_the_header_address`,
  `autocrypt_trust_applies_per_address`; `tests/roundtrip.rs` signer
  binding for signed and encrypted mail), `nail-store` (peers by
  address), `nail-mime` (suggestion binding), `nail-mcp` (delegation),
  `nail-core/tests/security.rs` (`mcp_encrypted_gate_follows_the_reader`
  with the four wrappings, `encrypted_drafts_never_reach_the_server_drafts_folder`,
  `authserv_ids_are_trusted_only_when_configured`), Vitest (account
  settings, security mock) and the Playwright scenario "sender checks
  count only for a receiving server the user configured".
