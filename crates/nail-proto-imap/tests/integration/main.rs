//! Integration tests of the IMAP client against the `nail-testkit` mock
//! server, built as one test binary so the shared helpers in [`common`] are
//! checked for dead code like any other module.
#![cfg(test)]

mod common;

mod bodies;
mod idle;
mod mock_server;
mod resync;
mod session;
mod writes;
