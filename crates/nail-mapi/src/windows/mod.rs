//! Everything that touches Win32 or caller memory. Every `unsafe` block of
//! the crate is in this module tree, each with its safety argument.
#![allow(unsafe_code)]

pub mod caller;
pub mod client;
pub mod decode;
pub mod exports;
pub(crate) mod memory;
pub mod server;
pub mod text;
