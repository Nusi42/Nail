//! `libsqlite3-sys` 0.38.2's API over the bundled SQLite3 Multiple
//! Ciphers amalgamation (Nail, ADR 0095). The Rust side is the
//! original crate's `lib.rs` / `error.rs` (MIT, the rusqlite developers).
#![expect(
    non_snake_case,
    non_camel_case_types,
    reason = "vendored bindgen output keeps SQLite's C names (sqlite3_int64, SQLITE_OK, ...)"
)]
#![cfg_attr(not(test), no_std)]

pub use self::error::*;

use core::mem;

mod error;

#[must_use]
pub fn SQLITE_STATIC() -> sqlite3_destructor_type {
    None
}

#[must_use]
pub fn SQLITE_TRANSIENT() -> sqlite3_destructor_type {
    Some(unsafe { mem::transmute::<isize, unsafe extern "C" fn(*mut core::ffi::c_void)>(-1_isize) })
}

mod bindings {
    include!(concat!(env!("OUT_DIR"), "/bindgen.rs"));
}
pub use bindings::*;

impl Default for sqlite3_vtab {
    fn default() -> Self {
        unsafe { mem::zeroed() }
    }
}

impl Default for sqlite3_vtab_cursor {
    fn default() -> Self {
        unsafe { mem::zeroed() }
    }
}
