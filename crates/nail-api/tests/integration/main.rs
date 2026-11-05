//! Integration tests of `nail-api`, built as one test binary so the crate and
//! its dependencies are linked once.
#![cfg(test)]

mod bindings;
mod catalog;
mod fixtures;
mod ts_vectors;
