//! Integration tests of `nail-server`, built as one test binary so the crate and
//! its dependencies are linked once and the shared helpers in
//! [`common`] are checked for dead code like any other module.
#![cfg(test)]

mod common;

mod remote_ui;
mod server;
mod tls_libs;
mod transport;
