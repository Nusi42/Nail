//! Reading the caller's memory without trusting it.
//!
//! Simple MAPI callers are arbitrary programs; a bad pointer must not take
//! their process down. Before the DLL reads a structure or a string it asks
//! `VirtualQuery` whether the bytes are committed and readable, scans
//! strings only up to a limit, and reads with `read_unaligned`. (The check
//! cannot stop another thread freeing the memory in between; nothing can,
//! short of structured exception handling Rust does not offer.)

use std::ffi::c_void;
use std::mem::{MaybeUninit, size_of};

use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
    PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE,
    PAGE_WRITECOPY, VirtualQuery,
};

/// Why a string could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StrError {
    /// The pointer leads into memory that is not readable.
    Unreadable,
    /// No terminator within the limit.
    TooLong,
}

const READABLE: u32 = PAGE_READONLY
    | PAGE_READWRITE
    | PAGE_WRITECOPY
    | PAGE_EXECUTE_READ
    | PAGE_EXECUTE_READWRITE
    | PAGE_EXECUTE_WRITECOPY;
const WRITABLE: u32 =
    PAGE_READWRITE | PAGE_WRITECOPY | PAGE_EXECUTE_READWRITE | PAGE_EXECUTE_WRITECOPY;

/// The end of the region containing `addr` when it is committed and has
/// one of the `wanted` protections.
fn region_end(addr: usize, wanted: u32) -> Option<usize> {
    let mut info = MaybeUninit::<MEMORY_BASIC_INFORMATION>::zeroed();
    // SAFETY: VirtualQuery only inspects the address space; `info` is a
    // writable buffer of the size passed.
    let written = unsafe {
        VirtualQuery(
            addr as *const c_void,
            info.as_mut_ptr(),
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    };
    if written == 0 {
        return None;
    }
    // SAFETY: VirtualQuery filled the structure (it returned its size).
    let info = unsafe { info.assume_init() };
    let protect = info.Protect;
    if info.State != MEM_COMMIT
        || protect & (PAGE_GUARD | PAGE_NOACCESS) != 0
        || protect & wanted == 0
    {
        return None;
    }
    let end = (info.BaseAddress as usize).checked_add(info.RegionSize)?;
    (end > addr).then_some(end)
}

fn accessible(ptr: *const u8, len: usize, wanted: u32) -> bool {
    if ptr.is_null() {
        return false;
    }
    let start = ptr as usize;
    let Some(end) = start.checked_add(len) else {
        return false;
    };
    let mut addr = start;
    while addr < end {
        match region_end(addr, wanted) {
            Some(next) => addr = next,
            None => return false,
        }
    }
    true
}

/// Whether `len` bytes at `ptr` can be read (`false` for null).
pub(crate) fn readable(ptr: *const u8, len: usize) -> bool {
    accessible(ptr, len, READABLE)
}

/// Whether `len` bytes at `ptr` can be written (`false` for null).
pub(crate) fn writable(ptr: *const u8, len: usize) -> bool {
    accessible(ptr, len, WRITABLE)
}

/// Copies a `T` from caller memory: `None` for null or unreadable
/// pointers. Unaligned pointers are fine.
pub(crate) fn read_struct<T: Copy>(ptr: *const T) -> Option<T> {
    if !readable(ptr.cast(), size_of::<T>()) {
        return None;
    }
    // SAFETY: the bytes are committed and readable (checked above); `T` is
    // a plain C structure (`Copy`, any bit pattern of its integer and
    // pointer fields is valid) and the read tolerates misalignment.
    Some(unsafe { std::ptr::read_unaligned(ptr) })
}

/// Writes a `T` to caller memory when it is writable.
pub(crate) fn write_struct<T: Copy>(ptr: *mut T, value: T) -> bool {
    if !writable(ptr.cast_const().cast(), size_of::<T>()) {
        return false;
    }
    // SAFETY: the bytes are committed and writable (checked above).
    unsafe { std::ptr::write_unaligned(ptr, value) };
    true
}

/// Reads a zero-terminated string of `T` units (bytes or UTF-16 units)
/// from caller memory, at most `max_units` long. `Ok(None)` for null.
pub(crate) fn read_units<T: Copy + Default + PartialEq>(
    ptr: *const T,
    max_units: usize,
) -> Result<Option<Vec<T>>, StrError> {
    if ptr.is_null() {
        return Ok(None);
    }
    let unit = size_of::<T>();
    let zero = T::default();
    let mut out = Vec::new();
    let mut addr = ptr as usize;
    // [ptr, known_end) is known to be readable.
    let mut known_end = addr;
    loop {
        let unit_end = addr.checked_add(unit).ok_or(StrError::Unreadable)?;
        while unit_end > known_end {
            known_end = region_end(known_end.max(addr), READABLE).ok_or(StrError::Unreadable)?;
        }
        // SAFETY: `addr..unit_end` lies in readable memory (checked above);
        // any bit pattern is a valid byte / UTF-16 unit.
        let value = unsafe { std::ptr::read_unaligned(addr as *const T) };
        if value == zero {
            return Ok(Some(out));
        }
        if out.len() >= max_units {
            return Err(StrError::TooLong);
        }
        out.push(value);
        addr = unit_end;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, VirtualAlloc, VirtualFree, VirtualProtect,
    };

    /// Two pages: the first readable and writable, the second no access —
    /// for strings and structures that run off the end of readable memory.
    pub(crate) struct GuardedPages {
        base: *mut u8,
        page: usize,
    }

    impl GuardedPages {
        pub(crate) fn new() -> Self {
            let page = 4096;
            // SAFETY: plain allocation of two fresh pages.
            let base = unsafe {
                VirtualAlloc(
                    std::ptr::null(),
                    page * 2,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                )
            }
            .cast::<u8>();
            assert!(!base.is_null());
            let mut old = 0;
            // SAFETY: the second page belongs to this allocation.
            let ok =
                unsafe { VirtualProtect(base.add(page).cast(), page, PAGE_NOACCESS, &raw mut old) };
            assert_ne!(ok, 0);
            Self { base, page }
        }

        /// A pointer `back` bytes before the inaccessible page.
        pub(crate) fn before_end(&self, back: usize) -> *mut u8 {
            // SAFETY: stays inside the first page.
            unsafe { self.base.add(self.page - back) }
        }

        /// The inaccessible page.
        pub(crate) fn no_access(&self) -> *mut u8 {
            // SAFETY: start of the second page of the allocation.
            unsafe { self.base.add(self.page) }
        }
    }

    impl Drop for GuardedPages {
        fn drop(&mut self) {
            // SAFETY: releases the allocation made in `new`.
            unsafe { VirtualFree(self.base.cast(), 0, MEM_RELEASE) };
        }
    }

    #[test]
    fn probes_and_reads() {
        // Static read-only data is readable but not writable.
        static TEXT: [u8; 4] = *b"abc\0";
        let value: u64 = 0x1122_3344_5566_7788;
        assert!(readable((&raw const value).cast(), 8));
        assert_eq!(read_struct(&raw const value), Some(value));
        assert!(!readable(std::ptr::null(), 1));
        assert!(!readable(0x10 as *const u8, 1));
        assert!(!readable(usize::MAX as *const u8, 2));
        assert_eq!(read_struct(0x10 as *const u64), None);
        assert!(readable(TEXT.as_ptr(), 4));
        assert!(!writable(TEXT.as_ptr(), 4));
        let mut slot = 0usize;
        assert!(write_struct(&raw mut slot, 7));
        assert_eq!(slot, 7);
        assert!(!write_struct(0x20 as *mut usize, 7));
    }

    #[test]
    fn strings_are_bounded() {
        let text = b"hello\0";
        assert_eq!(read_units(text.as_ptr(), 100), Ok(Some(b"hello".to_vec())));
        assert_eq!(read_units(text.as_ptr(), 5), Ok(Some(b"hello".to_vec())));
        assert_eq!(read_units(text.as_ptr(), 4), Err(StrError::TooLong));
        assert_eq!(read_units::<u8>(std::ptr::null(), 4), Ok(None));
        assert_eq!(read_units(0x30 as *const u16, 4), Err(StrError::Unreadable));
        let wide: Vec<u16> = "wörld\0".encode_utf16().collect();
        assert_eq!(
            read_units(wide.as_ptr(), 100)
                .unwrap()
                .map(|u| String::from_utf16(&u).unwrap()),
            Some("wörld".to_owned())
        );
    }

    #[expect(
        clippy::cast_ptr_alignment,
        reason = "deliberately misaligned and guard-page pointers: the readers must cope with both"
    )]
    #[test]
    fn unterminated_strings_at_the_end_of_memory() {
        let pages = GuardedPages::new();
        let start = pages.before_end(4);
        // SAFETY: four writable bytes before the guard page.
        unsafe { std::ptr::copy_nonoverlapping(b"abcd".as_ptr(), start, 4) };
        assert_eq!(
            read_units(start.cast_const(), 1000),
            Err(StrError::Unreadable)
        );
        // SAFETY: terminate inside the first page.
        unsafe { *start.add(3) = 0 };
        assert_eq!(
            read_units(start.cast_const(), 1000),
            Ok(Some(b"abc".to_vec()))
        );
        // A UTF-16 unit straddling the boundary.
        let odd = pages.before_end(1);
        // SAFETY: one writable byte.
        unsafe { *odd = 0x41 };
        assert_eq!(
            read_units(odd.cast_const().cast::<u16>(), 10),
            Err(StrError::Unreadable)
        );
        assert!(!readable(pages.before_end(4), 8));
        assert!(readable(pages.before_end(4), 4));
        assert_eq!(read_struct(pages.no_access().cast::<u32>()), None);
        assert_eq!(read_struct(pages.before_end(4).cast::<u64>()), None);
    }
}
