//! Integration tests of `nail-proto-smtp`, built as one test binary so the crate and
//! its dependencies are linked once.
#![cfg(test)]

mod oauth_send;
mod smtp;
