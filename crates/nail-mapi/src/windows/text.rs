//! Text conversions: `MAPISendMail` (the "A" function) passes strings in
//! the system's ANSI code page (`CP_ACP`, which is UTF-8 when the user
//! enabled "Beta: Use Unicode UTF-8"), `MAPISendMailW` passes UTF-16.

use windows_sys::Win32::Globalization::{CP_ACP, MultiByteToWideChar, WideCharToMultiByte};

/// ANSI bytes (no terminator) → text; invalid sequences become U+FFFD.
pub fn ansi_to_string(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let Ok(len) = i32::try_from(bytes.len()) else {
        return String::from_utf8_lossy(bytes).into_owned();
    };
    // SAFETY: `bytes` is a valid slice of `len` bytes; the first call only
    // measures.
    let needed =
        unsafe { MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), len, std::ptr::null_mut(), 0) };
    if needed <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut wide = vec![0u16; needed as usize];
    // SAFETY: `wide` has room for `needed` units.
    let written =
        unsafe { MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), len, wide.as_mut_ptr(), needed) };
    if written <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    wide.truncate(written as usize);
    String::from_utf16_lossy(&wide)
}

/// UTF-16 units (no terminator) → text; lone surrogates become U+FFFD.
pub fn wide_to_string(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

/// Text → zero-terminated UTF-16.
pub fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Text → zero-terminated ANSI bytes (characters the code page lacks
/// become `?`).
pub fn to_ansi(text: &str) -> Vec<u8> {
    let wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return vec![0];
    }
    let Ok(len) = i32::try_from(wide.len()) else {
        return vec![0];
    };
    // SAFETY: measuring call over a valid slice.
    let needed = unsafe {
        WideCharToMultiByte(
            CP_ACP,
            0,
            wide.as_ptr(),
            len,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    };
    if needed <= 0 {
        return vec![0];
    }
    let mut out = vec![0u8; needed as usize + 1];
    // SAFETY: `out` has room for `needed` bytes plus the terminator.
    let written = unsafe {
        WideCharToMultiByte(
            CP_ACP,
            0,
            wide.as_ptr(),
            len,
            out.as_mut_ptr(),
            needed,
            std::ptr::null(),
            std::ptr::null_mut(),
        )
    };
    out.truncate(written.max(0) as usize);
    out.push(0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn round_trips() {
        assert_eq!(ansi_to_string(b"Report Q3"), "Report Q3");
        assert_eq!(ansi_to_string(b""), "");
        assert_eq!(to_ansi("abc"), b"abc\0");
        assert_eq!(to_ansi(""), b"\0");
        assert_eq!(ansi_to_string(&to_ansi("Mail to Ada")[..11]), "Mail to Ada");
        assert_eq!(to_wide("ä"), vec![0xE4, 0]);
        assert_eq!(wide_to_string(&[0x48, 0x69]), "Hi");
        assert_eq!(wide_to_string(&[0xD800]), "\u{FFFD}");
    }

    #[test]
    fn code_page_characters() {
        // "ä" exists in every Western code page and in UTF-8; whatever the
        // system's code page, encoding and decoding give it back.
        let bytes = to_ansi("Grüße");
        let text = ansi_to_string(&bytes[..bytes.len() - 1]);
        assert!(text == "Grüße" || text.contains('?'), "{text}");
    }
}
