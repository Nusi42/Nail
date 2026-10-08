//! Folder names: sanitizing, special-use roles, IMAP modified UTF-7
//! (Maildir++ folder names), and safe file names for exports.

use base64::Engine;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use nail_api::domain::FolderRole;

use crate::sink::SourceFolder;

/// The deepest folder tree an importer reads (EML folders, Maildir,
/// Thunderbird `.sbd` trees).
pub const MAX_FOLDER_DEPTH: usize = 32;

/// A folder name Nail can create: no `/` (the path separator), no control
/// characters, trimmed, not empty. Returns the name and whether it was
/// changed.
pub fn sanitize_folder_name(name: &str) -> (String, bool) {
    let cleaned: String = name
        .chars()
        .map(|c| if c == '/' || c.is_control() { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim();
    let out = if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        "_".to_owned()
    } else {
        trimmed.to_owned()
    };
    let changed = out != name;
    (out, changed)
}

/// The special-use role of a folder by its (English, Thunderbird or
/// Maildir) name; only top-level folders get one.
pub fn guess_role(path: &str) -> FolderRole {
    if path.contains('/') {
        return FolderRole::Other;
    }
    match path.to_ascii_lowercase().as_str() {
        "inbox" => FolderRole::Inbox,
        "sent" | "sent items" | "sent messages" | "sent mail" => FolderRole::Sent,
        "drafts" | "draft" => FolderRole::Drafts,
        "archive" | "archives" => FolderRole::Archive,
        "junk" | "spam" | "junk e-mail" | "junk email" => FolderRole::Junk,
        "trash" | "deleted items" | "deleted messages" | "bin" => FolderRole::Trash,
        _ => FolderRole::Other,
    }
}

/// A [`SourceFolder`] from its path segments (each already a display
/// name); the name is sanitized. Returns the folder and whether its name
/// changed.
pub fn source_folder(segments: &[String]) -> (SourceFolder, bool) {
    let mut changed = false;
    let names: Vec<String> = segments
        .iter()
        .map(|s| {
            let (name, c) = sanitize_folder_name(s);
            changed |= c;
            name
        })
        .collect();
    let path = names.join("/");
    let parent = (names.len() > 1).then(|| names[..names.len() - 1].join("/"));
    let name = names.last().cloned().unwrap_or_else(|| "_".to_owned());
    (
        SourceFolder {
            role: guess_role(&path),
            path,
            name,
            parent,
        },
        changed,
    )
}

/// Whether `path` is selected by the import options' folder list (`None`
/// = all). A folder is selected when it or one of its parents is listed.
pub fn folder_selected(selection: Option<&[String]>, path: &str) -> bool {
    let Some(list) = selection else {
        return true;
    };
    list.iter().any(|wanted| {
        path == wanted
            || path
                .strip_prefix(wanted.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

// ---------------------------------------------------------------------------
// Modified UTF-7 (RFC 3501 §5.1.3)

fn mutf7_engine() -> GeneralPurpose {
    GeneralPurpose::new(
        &alphabet::IMAP_MUTF7,
        GeneralPurposeConfig::new()
            .with_encode_padding(false)
            .with_decode_padding_mode(DecodePaddingMode::RequireNone)
            .with_decode_allow_trailing_bits(true),
    )
}

/// Decodes an IMAP modified UTF-7 name; malformed parts are kept as
/// written.
pub fn decode_mutf7(name: &str) -> String {
    let engine = mutf7_engine();
    let mut out = String::new();
    let mut rest = name;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let Some(dash) = after.find('-') else {
            out.push_str(&rest[amp..]);
            return out;
        };
        let encoded = &after[..dash];
        if encoded.is_empty() {
            out.push('&');
        } else {
            let decoded = engine.decode(encoded).ok().and_then(|bytes| {
                if bytes.len() % 2 != 0 {
                    return None;
                }
                let units: Vec<u16> = bytes
                    .chunks(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16(&units).ok()
            });
            if let Some(text) = decoded {
                out.push_str(&text);
            } else {
                out.push('&');
                out.push_str(encoded);
                out.push('-');
            }
        }
        rest = &after[dash + 1..];
    }
    out.push_str(rest);
    out
}

/// Encodes a name as IMAP modified UTF-7.
pub fn encode_mutf7(name: &str) -> String {
    let engine = mutf7_engine();
    let mut out = String::new();
    let mut pending: Vec<u16> = Vec::new();
    let flush = |pending: &mut Vec<u16>, out: &mut String| {
        if pending.is_empty() {
            return;
        }
        let bytes: Vec<u8> = pending.iter().flat_map(|u| u.to_be_bytes()).collect();
        out.push('&');
        out.push_str(&engine.encode(bytes));
        out.push('-');
        pending.clear();
    };
    for c in name.chars() {
        if (' '..='~').contains(&c) {
            flush(&mut pending, &mut out);
            if c == '&' {
                out.push_str("&-");
            } else {
                out.push(c);
            }
        } else {
            let mut buf = [0u16; 2];
            pending.extend_from_slice(c.encode_utf16(&mut buf));
        }
    }
    flush(&mut pending, &mut out);
    out
}

// ---------------------------------------------------------------------------
// File names

/// A file / directory name safe on Windows and elsewhere: reserved
/// characters replaced, no trailing dots or spaces, no reserved device
/// names, at most `max_chars` characters. Never empty.
pub fn safe_file_name(name: &str, max_chars: usize) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .take(max_chars.max(1))
        .collect();
    while out.ends_with(['.', ' ']) {
        out.pop();
    }
    let out = out.trim_start().to_owned();
    if out.is_empty() {
        return "_".to_owned();
    }
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved { format!("_{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn names_are_sanitized() {
        assert_eq!(sanitize_folder_name("Inbox"), ("Inbox".into(), false));
        assert_eq!(sanitize_folder_name("a/b"), ("a_b".into(), true));
        assert_eq!(sanitize_folder_name(" x\u{7}"), ("x_".into(), true));
        assert_eq!(sanitize_folder_name(""), ("_".into(), true));
        assert_eq!(sanitize_folder_name(".."), ("_".into(), true));
    }

    #[test]
    fn roles() {
        assert_eq!(guess_role("INBOX"), FolderRole::Inbox);
        assert_eq!(guess_role("Sent Items"), FolderRole::Sent);
        assert_eq!(guess_role("Archives"), FolderRole::Archive);
        assert_eq!(guess_role("Archives/Trash"), FolderRole::Other);
        assert_eq!(guess_role("Lists"), FolderRole::Other);
    }

    #[test]
    fn source_folders_from_segments() {
        let (folder, changed) = source_folder(&["Archives".into(), "2024".into()]);
        assert_eq!(folder.path, "Archives/2024");
        assert_eq!(folder.parent.as_deref(), Some("Archives"));
        assert_eq!(folder.name, "2024");
        assert!(!changed);
        let (folder, changed) = source_folder(&["a/b".into()]);
        assert_eq!(folder.path, "a_b");
        assert!(changed);
        assert_eq!(folder.parent, None);
    }

    #[test]
    fn selection_includes_subfolders() {
        let list = vec!["Archives".to_owned()];
        assert!(folder_selected(None, "x"));
        assert!(folder_selected(Some(&list), "Archives"));
        assert!(folder_selected(Some(&list), "Archives/2024"));
        assert!(!folder_selected(Some(&list), "Archives2"));
        assert!(!folder_selected(Some(&list), "Inbox"));
    }

    #[test]
    fn modified_utf7_round_trips() {
        for name in [
            "Entwürfe",
            "Gesendete Objekte",
            "R&D",
            "日本語",
            "a&b ü €",
            "😀x",
        ] {
            let encoded = encode_mutf7(name);
            assert!(encoded.is_ascii(), "{encoded}");
            assert_eq!(decode_mutf7(&encoded), name);
        }
        assert_eq!(encode_mutf7("Entwürfe"), "Entw&APw-rfe");
        assert_eq!(decode_mutf7("&ZeVnLIqe-"), "日本語");
        assert_eq!(decode_mutf7("R&-D"), "R&D");
        // Malformed parts stay as written.
        assert_eq!(decode_mutf7("a&b"), "a&b");
        assert_eq!(decode_mutf7("a&A-b"), "a&A-b");
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(safe_file_name("Re: a/b?", 100), "Re_ a_b_");
        assert_eq!(safe_file_name("x. ", 100), "x");
        assert_eq!(safe_file_name("", 100), "_");
        assert_eq!(safe_file_name("CON", 100), "_CON");
        assert_eq!(safe_file_name("com1.txt", 100), "_com1.txt");
        assert_eq!(safe_file_name("COMPUTER", 100), "COMPUTER");
        assert_eq!(safe_file_name("abcdef", 3), "abc");
    }
}
