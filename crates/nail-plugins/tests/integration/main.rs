//! Integration tests of `nail-plugins`, built as one test binary so the crate and
//! its dependencies are linked once and the shared helpers in
//! [`common`] are checked for dead code like any other module.
#![cfg(test)]

mod common;

mod api;
mod dev;
mod extensions;
mod hooks;
mod jobs;
mod sandbox;
mod stop_and_logs;
mod writes;
