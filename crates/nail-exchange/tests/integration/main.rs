//! Integration tests of `nail-exchange`, built as one test binary so the crate and
//! its dependencies are linked once.
#![cfg(test)]

mod autodiscover;
mod libraries;
mod sspi_live;
