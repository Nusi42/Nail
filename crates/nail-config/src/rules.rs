//! `rules.kdl`: filter rules (M1b WP22).
//!
//! Empty until WP22: this module parses `rules.kdl` into
//! [`nail_api::rules::Rule`]s (every error of the file, with positions) and
//! writes rules back without losing comments or formatting, like
//! [`crate::AccountsDocument`] does for `accounts.kdl`. It is declared in
//! `lib.rs` already so WP22 does not need to edit that file (ADR 0022);
//! WP22 adds its public items to the re-exports.
