//! Integration tests of `nail-itip`, built as one test binary so the crate and
//! its dependencies are linked once and the shared helpers in
//! [`common`] are checked for dead code like any other module.
#![cfg(test)]

mod common;

mod interop;
mod libraries;
