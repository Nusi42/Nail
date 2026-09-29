//! `Authentication-Results` (RFC 8601) → [`nail_api::security::AuthResults`]
//! (M2a WP33; empty after WP30, ADR 0029).
//!
//! Expected shape: parse every `Authentication-Results` header (and
//! `ARC-Authentication-Results` for ARC), keep only those whose
//! `authserv-id` the account trusts (configured, or learned from the
//! account's own receiving server), and map DKIM / SPF / DMARC / ARC
//! results with their properties (`header.d`, `header.s`, `smtp.mailfrom`,
//! `header.from`) and alignment with the `From` domain. Anyone can write
//! such a header, so an untrusted one must never produce a verdict.
