//! Mailbox listing: roles (RFC 6154 SPECIAL-USE with name heuristics) and
//! modified UTF-7 names (RFC 3501 section 5.1.3).

use imap_proto::types::NameAttribute;
use nail_api::FolderRole;

/// A mailbox as listed by the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteFolder {
    /// Full server path as sent by the server (modified UTF-7); use this for
    /// commands such as [`crate::ImapClient::examine`].
    pub path: String,
    /// Display name: the last path segment, decoded.
    pub name: String,
    /// Hierarchy delimiter, `None` for a flat namespace.
    pub delimiter: Option<String>,
    /// Path of the parent mailbox, `None` for top-level mailboxes.
    pub parent_path: Option<String>,
    /// Role from SPECIAL-USE attributes, else from well-known names.
    pub role: FolderRole,
    /// `false` for `\Noselect` / `\NonExistent` containers.
    pub selectable: bool,
}

impl RemoteFolder {
    pub(crate) fn from_list(
        path: &str,
        delimiter: Option<&str>,
        attributes: &[NameAttribute<'_>],
    ) -> Self {
        let delimiter = delimiter.filter(|d| !d.is_empty()).map(str::to_owned);
        let (parent_path, leaf) = match delimiter.as_deref().and_then(|d| path.rsplit_once(d)) {
            Some((parent, leaf)) if !parent.is_empty() => (Some(parent.to_owned()), leaf),
            _ => (None, path),
        };
        let selectable = !attributes.iter().any(|attr| match attr {
            NameAttribute::NoSelect => true,
            NameAttribute::Extension(ext) => ext.eq_ignore_ascii_case("\\NonExistent"),
            _ => false,
        });
        let name = decode_modified_utf7(leaf);
        let role = special_use_role(attributes)
            .or_else(|| {
                path.eq_ignore_ascii_case("INBOX")
                    .then_some(FolderRole::Inbox)
            })
            .or_else(|| role_from_name(&name))
            .unwrap_or(FolderRole::Other);
        Self {
            path: path.to_owned(),
            name,
            delimiter,
            parent_path,
            role,
            selectable,
        }
    }
}

fn special_use_role(attributes: &[NameAttribute<'_>]) -> Option<FolderRole> {
    attributes.iter().find_map(|attr| match attr {
        NameAttribute::Sent => Some(FolderRole::Sent),
        NameAttribute::Drafts => Some(FolderRole::Drafts),
        NameAttribute::Archive => Some(FolderRole::Archive),
        NameAttribute::Junk => Some(FolderRole::Junk),
        NameAttribute::Trash => Some(FolderRole::Trash),
        _ => None,
    })
}

/// Guesses a role from common folder names (English and German; servers
/// without SPECIAL-USE).
pub fn role_from_name(name: &str) -> Option<FolderRole> {
    let role = match name.trim().to_lowercase().as_str() {
        "inbox" | "posteingang" => FolderRole::Inbox,
        "sent" | "sent items" | "sent mail" | "sent messages" | "gesendet"
        | "gesendete elemente" | "gesendete objekte" => FolderRole::Sent,
        "drafts" | "draft" | "entwürfe" | "entwurf" => FolderRole::Drafts,
        "archive" | "archives" | "archiv" => FolderRole::Archive,
        "junk" | "spam" | "junk e-mail" | "junk email" | "bulk mail" | "junk-e-mail" => {
            FolderRole::Junk
        }
        "trash"
        | "deleted items"
        | "deleted messages"
        | "bin"
        | "papierkorb"
        | "gelöschte elemente"
        | "gelöschte objekte" => FolderRole::Trash,
        _ => return None,
    };
    Some(role)
}

/// Encodes a mailbox name as IMAP modified UTF-7 (`Entwürfe` →
/// `Entw&APw-rfe`, `&` → `&-`).
pub fn encode_modified_utf7(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut pending: Vec<u16> = Vec::new();
    for c in input.chars() {
        if (' '..='~').contains(&c) {
            flush_utf16(&mut out, &mut pending);
            if c == '&' {
                out.push_str("&-");
            } else {
                out.push(c);
            }
        } else {
            let mut units = [0u16; 2];
            pending.extend_from_slice(c.encode_utf16(&mut units));
        }
    }
    flush_utf16(&mut out, &mut pending);
    out
}

fn flush_utf16(out: &mut String, pending: &mut Vec<u16>) {
    if pending.is_empty() {
        return;
    }
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,";
    let bytes: Vec<u8> = pending.drain(..).flat_map(u16::to_be_bytes).collect();
    out.push('&');
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let symbols = chunk.len() + 1;
        for i in 0..symbols {
            let index = (n >> (18 - 6 * i)) & 0x3f;
            out.push(char::from(ALPHABET[index as usize]));
        }
    }
    out.push('-');
}

/// Builds the server path of a new mailbox named `name` (display form)
/// below `parent` (a server path, `None` for top level), using the
/// hierarchy `delimiter` from `LIST`. Fails when `name` is empty or
/// contains the delimiter or control characters.
pub fn folder_path(
    parent: Option<&str>,
    delimiter: Option<&str>,
    name: &str,
) -> crate::Result<String> {
    let invalid = name.trim().is_empty()
        || name.chars().any(char::is_control)
        || delimiter.is_some_and(|d| !d.is_empty() && name.contains(d));
    if invalid {
        return Err(crate::ImapError::InvalidArgument(format!(
            "{name:?} is not a valid folder name"
        )));
    }
    let leaf = encode_modified_utf7(name);
    match (parent, delimiter) {
        (Some(parent), Some(delimiter)) if !parent.is_empty() => {
            Ok(format!("{parent}{delimiter}{leaf}"))
        }
        (Some(parent), None) if !parent.is_empty() => Err(crate::ImapError::InvalidArgument(
            "the server has no folder hierarchy".to_owned(),
        )),
        _ => Ok(leaf),
    }
}

/// Decodes an IMAP modified UTF-7 mailbox name (`Entw&APw-rfe` → `Entwürfe`).
/// Invalid sequences are kept verbatim.
pub fn decode_modified_utf7(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let Some(end) = after.find('-') else {
            out.push_str(&rest[amp..]);
            return out;
        };
        let encoded = &after[..end];
        if encoded.is_empty() {
            out.push('&');
        } else {
            match decode_base64_utf16(encoded) {
                Some(decoded) => out.push_str(&decoded),
                None => out.push_str(&rest[amp..amp + 1 + end + 1]),
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn decode_base64_utf16(encoded: &str) -> Option<String> {
    let mut bits: u32 = 0;
    let mut nbits = 0;
    let mut bytes = Vec::new();
    for c in encoded.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b',' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(value);
        nbits += 6;
        if nbits >= 8 {
            nbits -= 8;
            bytes.push(((bits >> nbits) & 0xff) as u8);
        }
    }
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|p| u16::from_be_bytes([p[0], p[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modified_utf7() {
        assert_eq!(decode_modified_utf7("INBOX"), "INBOX");
        assert_eq!(decode_modified_utf7("Entw&APw-rfe"), "Entwürfe");
        assert_eq!(
            decode_modified_utf7("Gel&APY-schte Elemente"),
            "Gelöschte Elemente"
        );
        assert_eq!(decode_modified_utf7("Tom &- Jerry"), "Tom & Jerry");
        assert_eq!(decode_modified_utf7("&ZeVnLIqe-"), "日本語");
        assert_eq!(decode_modified_utf7("broken&"), "broken&");
        assert_eq!(decode_modified_utf7("bad&!!-x"), "bad&!!-x");
    }

    #[test]
    fn modified_utf7_encoding_round_trips() {
        for (plain, encoded) in [
            ("INBOX", "INBOX"),
            ("Entwürfe", "Entw&APw-rfe"),
            ("Gelöschte Elemente", "Gel&APY-schte Elemente"),
            ("Tom & Jerry", "Tom &- Jerry"),
            ("日本語", "&ZeVnLIqe-"),
            ("😀", "&2D3eAA-"),
        ] {
            assert_eq!(encode_modified_utf7(plain), encoded, "{plain}");
            assert_eq!(decode_modified_utf7(encoded), plain, "{encoded}");
        }
    }

    #[test]
    fn folder_paths() {
        assert_eq!(
            folder_path(None, Some("/"), "Entwürfe").unwrap(),
            "Entw&APw-rfe"
        );
        assert_eq!(
            folder_path(Some("INBOX"), Some("."), "Work").unwrap(),
            "INBOX.Work"
        );
        assert!(folder_path(Some("INBOX"), Some("."), "a.b").is_err());
        assert!(folder_path(None, Some("/"), " ").is_err());
        assert!(
            folder_path(
                None,
                Some("/"),
                "a
b"
            )
            .is_err()
        );
        assert!(folder_path(Some("X"), None, "y").is_err());
        assert_eq!(folder_path(None, None, "y").unwrap(), "y");
    }

    #[test]
    fn roles_from_attributes_and_names() {
        let sent = RemoteFolder::from_list("Sent", Some("/"), &[NameAttribute::Sent]);
        assert_eq!(sent.role, FolderRole::Sent);
        let custom = RemoteFolder::from_list("Outgoing", Some("/"), &[NameAttribute::Sent]);
        assert_eq!(
            custom.role,
            FolderRole::Sent,
            "SPECIAL-USE wins over the name"
        );
        let inbox = RemoteFolder::from_list("inbox", Some("/"), &[]);
        assert_eq!(inbox.role, FolderRole::Inbox);
        let trash = RemoteFolder::from_list("INBOX.Gel&APY-schte Elemente", Some("."), &[]);
        assert_eq!(trash.role, FolderRole::Trash);
        assert_eq!(trash.name, "Gelöschte Elemente");
        assert_eq!(trash.parent_path.as_deref(), Some("INBOX"));
        let other = RemoteFolder::from_list("Receipts", None, &[]);
        assert_eq!(other.role, FolderRole::Other);
        assert_eq!(other.parent_path, None);
    }

    #[test]
    fn noselect_containers() {
        let folder = RemoteFolder::from_list("Work", Some("/"), &[NameAttribute::NoSelect]);
        assert!(!folder.selectable);
        let folder = RemoteFolder::from_list("Work/2026", Some("/"), &[]);
        assert!(folder.selectable);
        assert_eq!(folder.name, "2026");
        assert_eq!(folder.parent_path.as_deref(), Some("Work"));
    }
}
