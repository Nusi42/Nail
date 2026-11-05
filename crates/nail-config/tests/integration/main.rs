//! Integration tests of `nail-config`, built as one test binary so the crate and
//! its dependencies are linked once and the shared helpers in
//! [`common`] are checked for dead code like any other module.
#![cfg(test)]

mod common;

mod accounts;
mod appearance;
mod customize;
mod defaults;
mod errors;
mod exchange_accounts;
mod gmail_accounts;
mod layout;
mod layout_edit;
mod m5b_migration;
mod m5c_windows;
mod m5d_contract;
mod manager;
mod merge;
mod pim_sources;
mod rules;
mod rules_docs;
mod rules_schema;
mod rules_writer;
mod schema;
mod theme;
mod theme_contrast;
mod theme_edit;
mod user_docs;
mod write_back;
