//! Integration tests of `nail-store`, built as one test binary so the crate and
//! its dependencies are linked once and the shared helpers in
//! [`common`] are checked for dead code like any other module.
#![cfg(test)]

mod common;

mod bodies;
mod conversations;
mod crypto;
mod dbfile;
mod filter;
mod folder_ops;
mod mail;
mod queue;
mod queue_groups;
mod queue_v2;
mod remote;
mod seal;
mod sync_errors;
mod threads;
mod triage;
mod upgrade;
