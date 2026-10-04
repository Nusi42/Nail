//! Integration tests of the OpenPGP layer (Autocrypt, GnuPG interop and
//! round trips), built as one test binary so the shared helpers in
//! [`common`] are checked for dead code like any other module.
#![cfg(test)]

mod common;

mod autocrypt;
mod interop;
mod roundtrip;
