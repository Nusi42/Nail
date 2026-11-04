//! File names safe on Windows and elsewhere, made from names in mail and
//! folders: reserved and control characters replaced, no leading spaces, no
//! trailing dots or spaces, no reserved device names, a bounded length.
//!
//! [`safe_name`] cuts a long name at its end (folder and file names of an
//! export, where only the start matters); [`safe_attachment_name`] keeps
//! the extension of a long file name, so the system still knows its type
//! (opened attachments, Simple MAPI staging).

/// The name of an attachment without a usable one.
pub const FALLBACK_ATTACHMENT_NAME: &str = "attachment";

/// Longest extension (with its dot) that [`safe_attachment_name`] keeps
/// when it shortens a name; a longer "extension" is part of the stem.
pub const MAX_KEPT_EXTENSION_CHARS: usize = 16;

/// `name` with the characters Windows refuses in a file name (and control
/// characters) replaced by `_`.
fn replace_reserved(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// Whether Windows reads `name` as a device (`CON`, `NUL.txt`, `COM1`, …).
fn is_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default().trim_end();
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ((upper.starts_with("COM") || upper.starts_with("LPT"))
        && upper.len() == 4
        && upper.as_bytes()[3].is_ascii_digit())
}

/// `name` with a `_` in front when it is a device name.
fn undevice(name: String) -> String {
    if is_device_name(&name) {
        format!("_{name}")
    } else {
        name
    }
}

/// A safe name of at most `max_chars` characters (plus a `_` in front of a
/// device name), cut at its end; `_` when nothing is left.
pub fn safe_name(name: &str, max_chars: usize) -> String {
    let mut out: String = replace_reserved(name)
        .chars()
        .take(max_chars.max(1))
        .collect();
    while out.ends_with(['.', ' ']) {
        out.pop();
    }
    let out = out.trim_start().to_owned();
    if out.is_empty() {
        return "_".to_owned();
    }
    undevice(out)
}

/// A safe file name of at most `max_chars` characters: a longer name loses
/// the end of its stem and keeps its extension (up to
/// [`MAX_KEPT_EXTENSION_CHARS`]); [`FALLBACK_ATTACHMENT_NAME`] when nothing
/// is left.
pub fn safe_attachment_name(name: &str, max_chars: usize) -> String {
    let max_chars = max_chars.max(1);
    let replaced = replace_reserved(name);
    let trimmed = replaced.trim_start().trim_end_matches(['.', ' ']);
    if trimmed.is_empty() {
        return FALLBACK_ATTACHMENT_NAME.to_owned();
    }
    let out = undevice(trimmed.to_owned());
    if out.chars().count() <= max_chars {
        return out;
    }
    let extension = out
        .rfind('.')
        .filter(|&dot| dot > 0)
        .map(|dot| &out[dot..])
        .filter(|ext| {
            let chars = ext.chars().count();
            chars <= MAX_KEPT_EXTENSION_CHARS && chars < max_chars
        })
        .unwrap_or_default();
    let keep = max_chars - extension.chars().count();
    let head: String = out.chars().take(keep).collect();
    let head = head.trim_end_matches(['.', ' ']);
    if head.is_empty() {
        return FALLBACK_ATTACHMENT_NAME.to_owned();
    }
    format!("{head}{extension}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_made_safe() {
        assert_eq!(safe_name("Re: a/b?", 100), "Re_ a_b_");
        assert_eq!(safe_name("x. ", 100), "x");
        assert_eq!(safe_name("", 100), "_");
        assert_eq!(safe_name("CON", 100), "_CON");
        assert_eq!(safe_name("com1.txt", 100), "_com1.txt");
        assert_eq!(safe_name("COMPUTER", 100), "COMPUTER");
        assert_eq!(safe_name("conin$", 100), "_conin$");
        // Cut at the end, extension or not.
        assert_eq!(safe_name("abcdef", 3), "abc");
        assert_eq!(safe_name("abcdef.pdf", 6), "abcdef");
        assert_eq!(safe_name("del\u{7f}", 10), "del_");
    }

    #[test]
    fn attachment_names_are_made_safe() {
        assert_eq!(
            safe_attachment_name("Q3 report.docx", 200),
            "Q3 report.docx"
        );
        assert_eq!(
            safe_attachment_name(r#"a<b>c:d"e/f\g|h?i*j"#, 200),
            "a_b_c_d_e_f_g_h_i_j"
        );
        assert_eq!(safe_attachment_name("tab\there", 200), "tab_here");
        assert_eq!(safe_attachment_name("name. . ", 200), "name");
        assert_eq!(safe_attachment_name("  ", 200), FALLBACK_ATTACHMENT_NAME);
        assert_eq!(safe_attachment_name("...", 200), FALLBACK_ATTACHMENT_NAME);
        assert_eq!(safe_attachment_name("con.txt", 200), "_con.txt");
        assert_eq!(safe_attachment_name("COM1", 200), "_COM1");
        assert_eq!(safe_attachment_name("COM10.txt", 200), "COM10.txt");
        assert_eq!(safe_attachment_name("Consent.pdf", 200), "Consent.pdf");
    }

    #[test]
    fn a_long_attachment_name_keeps_its_extension() {
        let long = format!("{}.pdf", "x".repeat(300));
        let safe = safe_attachment_name(&long, 120);
        assert_eq!(safe.chars().count(), 120);
        assert_eq!(safe, format!("{}.pdf", "x".repeat(116)));
        // Characters, not bytes.
        let umlauts = format!("{}.tar.gz", "ä".repeat(250));
        let safe = safe_attachment_name(&umlauts, 120);
        assert_eq!(safe.chars().count(), 120);
        assert!(safe.ends_with("ä.gz"), "{safe}");
        // No extension, or one too long to be one: cut at the end.
        assert_eq!(safe_attachment_name(&"y".repeat(130), 120), "y".repeat(120));
        let odd = format!("a.{}", "z".repeat(200));
        assert_eq!(safe_attachment_name(&odd, 120).chars().count(), 120);
        // A dot in front is not an extension.
        assert_eq!(
            safe_attachment_name(&format!(".{}", "h".repeat(10)), 5),
            ".hhhh"
        );
        // The stem never ends in a dot or a space before the extension.
        let spaced = format!("{} .txt", "s".repeat(4));
        assert_eq!(safe_attachment_name(&spaced, 8), "ssss.txt");
    }
}
