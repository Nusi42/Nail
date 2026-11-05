//! Integration tests of `nail-dav`, built as one test binary so the crate and
//! its dependencies are linked once and the shared helpers in
//! [`common`] are checked for dead code like any other module.
#![cfg(test)]

mod common;

mod libraries;
mod mock_server;
mod scheduling;
