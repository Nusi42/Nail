//! The mail corpus: realistic, malformed and hostile `.eml` files.
//!
//! The corpus lives in `crates/nail-testkit/corpus/<folder>/*.eml`. Each
//! folder may contain a `flags.txt` with lines `<file> <IMAP flags...>`
//! (for example `01-welcome.eml \Seen \Flagged`); `#` starts a comment.
//! The files are embedded at build time, so [`demo_corpus`] also works in a
//! shipped binary (`nail serve --seed demo`).
//!
//! Message bytes are normalized to CRLF line endings, like on an IMAP server.
//!
//! `corpus/crypto/{pgp,smime}/` holds end-to-end test vectors (keys,
//! certificates, signed and encrypted messages made by GnuPG, Thunderbird
//! and OpenSSL; M2a, ADR 0029). They are not demo mail: [`demo_corpus`]
//! and [`load_mail_dir`] skip the directory; tests read the files with
//! [`crypto_fixture`].

use std::io;
use std::path::{Path, PathBuf};

use nail_api::{Flags, FolderRole};

struct EmbeddedFolder {
    name: &'static str,
    flags: &'static str,
    messages: &'static [(&'static str, &'static [u8])],
}

static EMBEDDED: &[EmbeddedFolder] = include!(concat!(env!("OUT_DIR"), "/corpus.rs"));

/// One message of a corpus folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusMessage {
    /// File name within the folder, e.g. `01-welcome.eml`.
    pub file_name: String,
    /// Raw RFC 5322 bytes with CRLF line endings.
    pub raw: Vec<u8>,
    /// IMAP flags as atoms, e.g. `\Seen`.
    pub flags: Vec<String>,
}

impl CorpusMessage {
    /// The standard system flags of this message as the API type.
    pub fn api_flags(&self) -> Flags {
        flags_from_imap(&self.flags)
    }
}

/// A folder of the corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusFolder {
    /// Folder (mailbox) name, e.g. `INBOX`, `Sent`, `Archive`.
    pub name: String,
    /// Special-use role derived from the name.
    pub role: FolderRole,
    /// Messages in file-name order (which is also date/UID order).
    pub messages: Vec<CorpusMessage>,
}

/// The embedded demo corpus: `INBOX` first, then the other folders by name.
pub fn demo_corpus() -> Vec<CorpusFolder> {
    let mut folders: Vec<CorpusFolder> = EMBEDDED
        .iter()
        .map(|folder| {
            let flags = parse_flags_file(folder.flags);
            CorpusFolder {
                name: folder.name.to_owned(),
                role: role_for_name(folder.name),
                messages: folder
                    .messages
                    .iter()
                    .map(|(file_name, raw)| CorpusMessage {
                        file_name: (*file_name).to_owned(),
                        raw: to_crlf(raw),
                        flags: flags_for(&flags, file_name),
                    })
                    .collect(),
            }
        })
        .collect();
    sort_folders(&mut folders);
    folders
}

/// The demo corpus as plain tuples `(folder name, role, raw messages)`, for
/// seeding a store without depending on the corpus types.
pub fn demo_corpus_raw() -> Vec<(String, FolderRole, Vec<Vec<u8>>)> {
    demo_corpus()
        .into_iter()
        .map(|folder| {
            let raws = folder.messages.into_iter().map(|m| m.raw).collect();
            (folder.name, folder.role, raws)
        })
        .collect()
}

/// Looks up one embedded corpus message by folder and file name.
pub fn corpus_message(folder: &str, file_name: &str) -> Option<Vec<u8>> {
    EMBEDDED
        .iter()
        .find(|f| f.name == folder)?
        .messages
        .iter()
        .find(|(name, _)| *name == file_name)
        .map(|(_, raw)| to_crlf(raw))
}

/// Path of the corpus directory in the source tree (for tests that want to
/// exercise [`load_mail_dir`]).
pub fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus")
}

/// Name of the directory of end-to-end test vectors inside the corpus.
pub const CRYPTO_FIXTURE_DIR: &str = "crypto";

/// The kinds of end-to-end test vectors (sub-directories of
/// [`CRYPTO_FIXTURE_DIR`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CryptoFixtures {
    /// `corpus/crypto/pgp/` (M2a WP31): OpenPGP keys and messages.
    Pgp,
    /// `corpus/crypto/smime/` (M2a WP32): certificates, PKCS#12 files and
    /// S/MIME messages.
    Smime,
}

impl CryptoFixtures {
    fn dir_name(self) -> &'static str {
        match self {
            Self::Pgp => "pgp",
            Self::Smime => "smime",
        }
    }
}

/// Directory of one kind of end-to-end test vectors in the source tree.
pub fn crypto_fixture_dir(kind: CryptoFixtures) -> PathBuf {
    corpus_dir().join(CRYPTO_FIXTURE_DIR).join(kind.dir_name())
}

/// The bytes of an end-to-end test vector, e.g.
/// `crypto_fixture(CryptoFixtures::Pgp, "alice.pub.asc")`. `None` when the
/// file does not exist. Line endings are kept as checked in.
pub fn crypto_fixture(kind: CryptoFixtures, file_name: &str) -> Option<Vec<u8>> {
    std::fs::read(crypto_fixture_dir(kind).join(file_name)).ok()
}

/// Loads a directory laid out like the corpus: one sub-directory per folder,
/// `.eml` files inside (sorted by file name), optional `flags.txt`. A
/// sub-directory named [`CRYPTO_FIXTURE_DIR`] is skipped.
pub fn load_mail_dir(dir: &Path) -> io::Result<Vec<CorpusFolder>> {
    let mut folders = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if !path.is_dir() || path.ends_with(CRYPTO_FIXTURE_DIR) {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let flags = match std::fs::read_to_string(path.join("flags.txt")) {
            Ok(text) => parse_flags_file(&text),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(err) => return Err(err),
        };
        let mut files: Vec<PathBuf> = std::fs::read_dir(&path)?
            .map(|e| e.map(|e| e.path()))
            .collect::<io::Result<_>>()?;
        files.retain(|p| {
            p.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("eml"))
        });
        files.sort();
        let mut messages = Vec::with_capacity(files.len());
        for file in files {
            let file_name = file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            messages.push(CorpusMessage {
                raw: to_crlf(&std::fs::read(&file)?),
                flags: flags_for(&flags, &file_name),
                file_name,
            });
        }
        folders.push(CorpusFolder {
            role: role_for_name(&name),
            name,
            messages,
        });
    }
    sort_folders(&mut folders);
    Ok(folders)
}

/// Maps well-known folder names to roles (the mock server advertises them as
/// SPECIAL-USE attributes).
pub fn role_for_name(name: &str) -> FolderRole {
    match name.to_ascii_lowercase().as_str() {
        "inbox" => FolderRole::Inbox,
        "sent" => FolderRole::Sent,
        "drafts" => FolderRole::Drafts,
        "archive" => FolderRole::Archive,
        "junk" | "spam" => FolderRole::Junk,
        "trash" => FolderRole::Trash,
        _ => FolderRole::Other,
    }
}

/// Converts IMAP flag atoms to the API's system flags (keywords are ignored).
pub fn flags_from_imap<S: AsRef<str>>(flags: &[S]) -> Flags {
    let mut out = Flags::default();
    for flag in flags {
        match flag.as_ref().to_ascii_lowercase().as_str() {
            "\\seen" => out.seen = true,
            "\\flagged" => out.flagged = true,
            "\\answered" => out.answered = true,
            "\\draft" => out.draft = true,
            _ => {}
        }
    }
    out
}

/// Normalizes line endings to CRLF (bare LF and bare CR become CRLF).
pub fn to_crlf(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + raw.len() / 32);
    let mut i = 0;
    while i < raw.len() {
        match raw[i] {
            b'\r' => {
                out.extend_from_slice(b"\r\n");
                if raw.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
            }
            b'\n' => out.extend_from_slice(b"\r\n"),
            byte => out.push(byte),
        }
        i += 1;
    }
    out
}

fn sort_folders(folders: &mut [CorpusFolder]) {
    folders.sort_by(|a, b| {
        (a.role != FolderRole::Inbox, &a.name).cmp(&(b.role != FolderRole::Inbox, &b.name))
    });
}

fn parse_flags_file(text: &str) -> Vec<(String, Vec<String>)> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let file = parts.next()?.to_owned();
            Some((file, parts.map(str::to_owned).collect()))
        })
        .collect()
}

fn flags_for(flags: &[(String, Vec<String>)], file_name: &str) -> Vec<String> {
    flags
        .iter()
        .find(|(name, _)| name == file_name)
        .map(|(_, f)| f.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_corpus_has_expected_folders() {
        let corpus = demo_corpus();
        let names: Vec<_> = corpus.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["INBOX", "Archive", "Sent"]);
        let roles: Vec<_> = corpus.iter().map(|f| f.role).collect();
        assert_eq!(
            roles,
            [FolderRole::Inbox, FolderRole::Archive, FolderRole::Sent]
        );
        let total: usize = corpus.iter().map(|f| f.messages.len()).sum();
        assert!((15..=25).contains(&total), "corpus has {total} messages");
    }

    #[test]
    fn crypto_fixtures_are_not_demo_mail() {
        assert!(crypto_fixture_dir(CryptoFixtures::Pgp).is_dir());
        assert!(crypto_fixture_dir(CryptoFixtures::Smime).is_dir());
        assert!(crypto_fixture(CryptoFixtures::Pgp, "README.md").is_some());
        assert!(crypto_fixture(CryptoFixtures::Smime, "missing.p12").is_none());
        let names: Vec<_> = load_mail_dir(&corpus_dir())
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert!(!names.iter().any(|n| n == CRYPTO_FIXTURE_DIR), "{names:?}");
    }

    #[test]
    fn embedded_corpus_matches_directory() {
        let from_dir = load_mail_dir(&corpus_dir()).unwrap();
        assert_eq!(from_dir, demo_corpus());
    }

    #[test]
    fn corpus_is_crlf_normalized() {
        for folder in demo_corpus() {
            for message in folder.messages {
                let raw = &message.raw;
                for (i, byte) in raw.iter().enumerate() {
                    if *byte == b'\n' {
                        assert_eq!(raw[i - 1], b'\r', "bare LF in {}", message.file_name);
                    }
                }
            }
        }
    }

    #[test]
    fn flags_are_applied() {
        let corpus = demo_corpus();
        let inbox = &corpus[0];
        let meeting = inbox
            .messages
            .iter()
            .find(|m| m.file_name == "03-alternative-meeting.eml")
            .unwrap();
        assert_eq!(meeting.flags, ["\\Seen", "\\Flagged"]);
        assert!(meeting.api_flags().seen && meeting.api_flags().flagged);
        let hostile = inbox
            .messages
            .iter()
            .find(|m| m.file_name == "09-hostile-html.eml")
            .unwrap();
        assert!(hostile.flags.is_empty());
    }

    #[test]
    fn raw_tuples_and_lookup() {
        let raw = demo_corpus_raw();
        assert_eq!(raw[0].0, "INBOX");
        assert_eq!(raw[0].1, FolderRole::Inbox);
        let welcome = corpus_message("INBOX", "01-welcome.eml").unwrap();
        assert_eq!(raw[0].2[0], welcome);
        assert!(corpus_message("INBOX", "nope.eml").is_none());
    }

    #[test]
    fn to_crlf_normalizes_all_line_endings() {
        assert_eq!(to_crlf(b"a\nb\r\nc\rd"), b"a\r\nb\r\nc\r\nd");
        assert_eq!(to_crlf(b""), b"");
    }

    #[test]
    fn role_names() {
        assert_eq!(role_for_name("inbox"), FolderRole::Inbox);
        assert_eq!(role_for_name("Spam"), FolderRole::Junk);
        assert_eq!(role_for_name("Receipts"), FolderRole::Other);
    }
}
