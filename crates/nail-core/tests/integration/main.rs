//! Integration tests of `nail-core`, built as one test binary so the crate and
//! its dependencies are linked once and the shared helpers in
//! [`common`] are checked for dead code like any other module.
#![cfg(test)]

mod common;

mod conversations;
mod engines;
mod exchange;
mod folders;
mod gmail;
mod launch;
mod m1b;
mod m2c;
mod m2c_wired;
mod m2d;
mod m3a;
mod m3b;
mod m3c;
mod m4a;
mod m4a_wired;
mod m4b;
mod m4c;
mod m4c_leftovers;
mod m4c_portability;
mod m4c_vault;
mod m5a;
mod mail_actions;
mod oauth;
mod pim;
mod pim_sync;
mod plugins;
mod remote;
mod restart;
mod scheduling;
mod security;
mod service;
mod snooze_restart;
mod sync;
