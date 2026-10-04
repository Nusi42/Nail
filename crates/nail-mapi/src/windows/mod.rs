//! Everything that touches Win32 or caller memory. Every `unsafe` block of
//! the crate is in this module tree, each with its safety argument.
#![expect(
    unsafe_code,
    reason = "Win32 calls and reads of caller memory, each with its safety argument"
)]

pub mod caller;
pub mod client;
pub mod decode;
pub mod exports;
pub(crate) mod memory;
pub mod server;
pub mod staging_root;
pub mod text;

/// `size_of::<T>()` as the `u32` that Win32 structure-size fields take
/// (every structure here is far smaller than 4 GiB).
pub(crate) fn size_u32<T>() -> u32 {
    u32::try_from(size_of::<T>()).unwrap_or(u32::MAX)
}

/// A length as the `u32` count MAPI and Win32 take (saturating).
pub(crate) fn count_u32(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}
