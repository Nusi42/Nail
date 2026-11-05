//! Integration tests of `nail-remote-auth`, built as one test binary so the crate and
//! its dependencies are linked once and the shared helpers in
//! [`common`] are checked for dead code like any other module.
#![cfg(test)]

mod common;

mod access;
mod admin;
mod http;
mod libraries;
mod pairing;
mod passkeys;
mod password;
mod sessions;
