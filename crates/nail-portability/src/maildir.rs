//! Maildir: the reader (`cur` / `new`, `:2,` flags, Dovecot
//! `dovecot-keywords` and Courier `courierimapkeywords/:list` keywords,
//! Maildir++ `.Sub.Folder` trees and nested `Sub/cur` trees) and the
//! writer (Maildir++).
//!
//! The info separator is `:` on Unix; Windows file names cannot hold it,
//! so `;` and `!` (used by Windows ports) are read too, and the writer
//! uses `;` on Windows.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use nail_api::Flags;
use nail_api::portability::{ItemRef, PortabilityIssueCode};

use crate::eml::sorted_entries;
use crate::error::{PortabilityError, Result};
use crate::files::require_folder;
use crate::folder::{decode_mutf7, encode_mutf7, sanitize_folder_name, source_folder};
use crate::import::{FolderLocation, MailFolderSource, RawMessage, read_message_file};
use crate::job::JobContext;
use crate::message::push_keyword;

/// Deepest folder tree read.
pub use crate::folder::MAX_FOLDER_DEPTH as MAX_DEPTH;

/// The info separators read (`:2,` and the Windows variants).
pub const INFO_SEPARATORS: [char; 3] = [':', ';', '!'];

/// The info separator the writer uses on this platform.
pub const WRITE_SEPARATOR: char = if cfg!(windows) { ';' } else { ':' };

/// One Maildir folder found by [`scan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaildirFolder {
    /// The folder's directory (holding `cur` / `new`).
    pub dir: PathBuf,
}

/// Whether `dir` is a maildir (has `cur` or `new`).
pub fn is_maildir(dir: &Path) -> bool {
    real_dir(&dir.join("cur")) || real_dir(&dir.join("new"))
}

/// A directory that is not a symbolic link or junction: a `cur` / `new`
/// pointing elsewhere must not import files from outside the chosen tree
/// (like [`sorted_entries`], which skips links).
fn real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir() && !m.file_type().is_symlink())
}

/// The folders of a Maildir: the root as `Inbox` (when it is a maildir),
/// Maildir++ `.A.B` folders as `A/B`, and nested `A/cur` folders; missing
/// parents are added without messages. Parents come first.
pub fn scan(root: &Path) -> Result<Vec<MailFolderSource>> {
    require_folder(root)?;
    let mut found: BTreeMap<Vec<String>, Option<PathBuf>> = BTreeMap::new();
    if is_maildir(root) {
        found.insert(vec!["Inbox".to_owned()], Some(root.to_path_buf()));
    }
    for (path, file_type) in sorted_entries(root)? {
        if !file_type.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(plus) = name.strip_prefix('.') {
            if plus.is_empty() || plus == "." || !is_maildir(&path) {
                continue;
            }
            let segments: Vec<String> = plus
                .split('.')
                .filter(|s| !s.is_empty())
                .map(decode_mutf7)
                .collect();
            if !segments.is_empty() {
                found.insert(segments, Some(path));
            }
        } else if !is_reserved(&name) {
            scan_nested(&path, &mut vec![decode_mutf7(&name)], 0, &mut found)?;
        }
    }
    if found.is_empty() {
        return Err(PortabilityError::InvalidSource(format!(
            "{} holds no maildir (no cur / new folder)",
            root.display()
        )));
    }
    // Missing parents.
    let keys: Vec<Vec<String>> = found.keys().cloned().collect();
    for key in keys {
        for len in 1..key.len() {
            found.entry(key[..len].to_vec()).or_insert(None);
        }
    }
    let mut out = Vec::new();
    for (segments, dir) in found {
        let (folder, renamed) = source_folder(&segments);
        let (messages, size_bytes) = dir.as_deref().map_or((0, 0), count);
        out.push(MailFolderSource {
            folder,
            renamed_from: renamed.then(|| segments.join("/")),
            location: match dir {
                Some(dir) => FolderLocation::Maildir(MaildirFolder { dir }),
                None => FolderLocation::Empty,
            },
            messages,
            size_bytes,
            estimated: false,
        });
    }
    Ok(out)
}

fn is_reserved(name: &str) -> bool {
    matches!(
        name,
        "cur" | "new" | "tmp" | "courierimapkeywords" | "courierimaphieracl" | "courierimapacl"
    )
}

fn scan_nested(
    dir: &Path,
    segments: &mut Vec<String>,
    depth: usize,
    found: &mut BTreeMap<Vec<String>, Option<PathBuf>>,
) -> Result<()> {
    if is_maildir(dir) {
        found.insert(segments.clone(), Some(dir.to_path_buf()));
    }
    if depth >= MAX_DEPTH {
        return Ok(());
    }
    for (path, file_type) in sorted_entries(dir)? {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if file_type.is_dir() && !name.starts_with('.') && !is_reserved(&name) {
            segments.push(decode_mutf7(&name));
            scan_nested(&path, segments, depth + 1, found)?;
            segments.pop();
        }
    }
    Ok(())
}

fn message_files(dir: &Path) -> Vec<(PathBuf, bool)> {
    let mut files = Vec::new();
    for (sub, is_new) in [("cur", false), ("new", true)] {
        let sub = dir.join(sub);
        if !real_dir(&sub) {
            continue;
        }
        let Ok(entries) = sorted_entries(&sub) else {
            continue;
        };
        for (path, file_type) in entries {
            let hidden = path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'));
            if file_type.is_file() && !hidden {
                files.push((path, is_new));
            }
        }
    }
    files
}

fn count(dir: &Path) -> (u64, u64) {
    message_files(dir)
        .iter()
        .fold((0, 0), |(n, size), (path, _)| {
            (n + 1, size + fs::metadata(path).map_or(0, |m| m.len()))
        })
}

/// Splits a file name into its base and its `2,` flags.
pub fn split_info(file_name: &str) -> (&str, Option<&str>) {
    for sep in INFO_SEPARATORS {
        let marker = format!("{sep}2,");
        if let Some(at) = file_name.rfind(&marker) {
            return (&file_name[..at], Some(&file_name[at + marker.len()..]));
        }
    }
    (file_name, None)
}

/// Flags of a Maildir info part.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InfoFlags {
    /// System flags.
    pub flags: Flags,
    /// `T`: trashed (to be expunged).
    pub trashed: bool,
    /// Keyword letters `a`–`z`.
    pub keyword_letters: Vec<char>,
    /// Letters Nail does not know.
    pub unknown: Vec<char>,
}

/// Reads the `2,` flags: `D` draft, `F` flagged, `P` passed (ignored), `R`
/// replied, `S` seen, `T` trashed, `a`–`z` keywords.
pub fn parse_info(info: &str) -> InfoFlags {
    let mut out = InfoFlags::default();
    for c in info.chars() {
        match c {
            'D' => out.flags.draft = true,
            'F' => out.flags.flagged = true,
            'P' => {}
            'R' => out.flags.answered = true,
            'S' => out.flags.seen = true,
            'T' => out.trashed = true,
            'a'..='z' => out.keyword_letters.push(c),
            _ => out.unknown.push(c),
        }
    }
    out
}

/// The keywords of a folder: Dovecot letters and Courier per-message lists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keywords {
    /// `dovecot-keywords`: letter index → keyword.
    pub dovecot: HashMap<u32, String>,
    /// Courier: message base name → keywords.
    pub courier: HashMap<String, Vec<String>>,
}

impl Keywords {
    /// Reads the keyword files of the folder at `dir`.
    pub fn read(dir: &Path) -> Self {
        let mut out = Self::default();
        if let Ok(text) = fs::read_to_string(dir.join("dovecot-keywords")) {
            for line in text.lines() {
                if let Some((index, keyword)) = line.split_once(' ')
                    && let Ok(index) = index.trim().parse::<u32>()
                    && !keyword.trim().is_empty()
                {
                    out.dovecot.insert(index, keyword.trim().to_owned());
                }
            }
        }
        if let Ok(text) = fs::read_to_string(dir.join("courierimapkeywords").join(":list")) {
            out.courier = parse_courier_list(&text);
        }
        out
    }

    /// The keywords of a message.
    pub fn of(&self, base: &str, letters: &[char]) -> (Vec<String>, Vec<char>) {
        let mut keywords = Vec::new();
        let mut unknown = Vec::new();
        for &letter in letters {
            let index = u32::from(letter) - u32::from('a');
            match self.dovecot.get(&index) {
                Some(keyword) => push_keyword(&mut keywords, keyword),
                None => unknown.push(letter),
            }
        }
        // Courier lists name messages by their base (without the info).
        let courier_base = base.split([':', ';', '!']).next().unwrap_or(base);
        if let Some(list) = self.courier.get(courier_base) {
            for keyword in list {
                push_keyword(&mut keywords, keyword);
            }
        }
        (keywords, unknown)
    }
}

/// Parses a Courier `:list` file: keyword names, an empty line, then
/// `base:index index …` lines.
pub fn parse_courier_list(text: &str) -> HashMap<String, Vec<String>> {
    let mut names = Vec::new();
    let mut lines = text.lines();
    for line in lines.by_ref() {
        if line.is_empty() {
            break;
        }
        names.push(line.to_owned());
    }
    let mut out = HashMap::new();
    for line in lines {
        let Some((base, indexes)) = line.rsplit_once(':') else {
            continue;
        };
        let keywords: Vec<String> = indexes
            .split_whitespace()
            .filter_map(|i| i.parse::<usize>().ok())
            .filter_map(|i| names.get(i).cloned())
            .collect();
        if !keywords.is_empty() {
            out.insert(base.to_owned(), keywords);
        }
    }
    out
}

/// The delivery time of a message file: the leading seconds of its name,
/// else the file's modification time.
fn received_at(path: &Path, base: &str) -> Option<i64> {
    let digits: String = base.chars().take_while(char::is_ascii_digit).collect();
    if digits.len() >= 9
        && let Ok(seconds) = digits.parse::<i64>()
    {
        return Some(seconds);
    }
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(nail_util::time::unix_secs_of)
}

/// Reads the messages of one Maildir folder (`cur`, then `new`).
pub fn read_folder(
    folder: &MaildirFolder,
    folder_path: &str,
    ctx: &mut JobContext<'_>,
    visit: &mut dyn FnMut(&mut JobContext<'_>, RawMessage) -> Result<()>,
) -> Result<()> {
    let keywords = Keywords::read(&folder.dir);
    for (index, (path, is_new)) in message_files(&folder.dir).into_iter().enumerate() {
        ctx.check_cancelled()?;
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let origin = ItemRef {
            file: Some(path.display().to_string()),
            folder: Some(folder_path.to_owned()),
            index: Some(index as u64 + 1),
            ..ItemRef::default()
        };
        let Some(raw) = read_message_file(&path, &origin, ctx, visit)? else {
            continue;
        };
        let (base, info) = split_info(&file_name);
        let parsed = info.map(parse_info).unwrap_or_default();
        let (message_keywords, unknown_letters) = keywords.of(base, &parsed.keyword_letters);
        let mut message = RawMessage::new(raw, origin);
        message.flags = parsed.flags;
        if is_new {
            message.flags.seen = false;
        }
        message.keywords = message_keywords;
        message.deleted = parsed.trashed;
        message.received_at = received_at(&path, base);
        for c in parsed.unknown.iter().chain(unknown_letters.iter()) {
            message.warnings.push((
                PortabilityIssueCode::UnknownFlag,
                format!("unknown Maildir flag {c:?} in {file_name}"),
            ));
        }
        visit(ctx, message)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Writing

static UNIQUE: AtomicU64 = AtomicU64::new(0);

/// Writes a Maildir++ tree: the root maildir and `.A.B` subfolders.
#[derive(Debug)]
pub struct MaildirWriter {
    root: PathBuf,
    /// Keywords per folder directory (letter order).
    keywords: HashMap<PathBuf, Vec<String>>,
}

/// What writing one message changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WriteNotes {
    /// Keywords dropped (more than 26 in the folder).
    pub dropped_keywords: Vec<String>,
}

impl MaildirWriter {
    /// A writer creating `root` as a maildir.
    pub fn create(root: &Path) -> Result<Self> {
        make_maildir(root)?;
        Ok(Self {
            root: root.to_path_buf(),
            keywords: HashMap::new(),
        })
    }

    /// The directory of a folder below the root (`A/B` → `.A.B`), created;
    /// `None` / empty = the root. Returns the directory and whether a name
    /// had to change (`.` is not allowed in Maildir++ names).
    pub fn folder(&mut self, path: Option<&str>) -> Result<(PathBuf, bool)> {
        let Some(path) = path.filter(|p| !p.is_empty()) else {
            return Ok((self.root.clone(), false));
        };
        let mut changed = false;
        let encoded: Vec<String> = path
            .split('/')
            .map(|segment| {
                let (name, c) = sanitize_folder_name(segment);
                changed |= c || name.contains('.');
                encode_mutf7(&name.replace('.', "_"))
            })
            .collect();
        let dir = self.root.join(format!(".{}", encoded.join(".")));
        make_maildir(&dir)?;
        let marker = dir.join("maildirfolder");
        if !marker.exists() {
            fs::write(&marker, b"")
                .map_err(|e| PortabilityError::io(marker.display().to_string(), e))?;
        }
        Ok((dir, changed))
    }

    /// Writes a message into the folder directory `dir` (from
    /// [`MaildirWriter::folder`]): to `tmp`, then renamed into `cur` with
    /// its flags.
    pub fn write(
        &mut self,
        dir: &Path,
        raw: &[u8],
        flags: Flags,
        keywords: &[String],
        received_at: Option<i64>,
    ) -> std::io::Result<(PathBuf, WriteNotes)> {
        let mut notes = WriteNotes::default();
        let mut letters = String::new();
        let known = self.keywords.entry(dir.to_path_buf()).or_default();
        for keyword in keywords {
            let index = match known.iter().position(|k| k.eq_ignore_ascii_case(keyword)) {
                Some(i) => i,
                None if known.len() < 26 => {
                    known.push(keyword.clone());
                    known.len() - 1
                }
                None => {
                    notes.dropped_keywords.push(keyword.clone());
                    continue;
                }
            };
            // At most 26 keywords per folder (checked above).
            let Some(letter) = u8::try_from(index).ok().map(|i| b'a' + i) else {
                continue;
            };
            letters.push(char::from(letter));
        }
        let mut info = String::new();
        if flags.draft {
            info.push('D');
        }
        if flags.flagged {
            info.push('F');
        }
        if flags.answered {
            info.push('R');
        }
        if flags.seen {
            info.push('S');
        }
        let mut sorted: Vec<char> = letters.chars().collect();
        sorted.sort_unstable();
        sorted.dedup();
        info.extend(sorted);
        let time = received_at.unwrap_or_else(nail_util::time::unix_now).max(0);
        let unique = UNIQUE.fetch_add(1, Ordering::Relaxed);
        let base = format!("{time}.M{unique}P{}Q{unique}.nail", std::process::id());
        let tmp = dir.join("tmp").join(&base);
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            file.write_all(raw)?;
            file.flush()?;
        }
        let target = dir
            .join("cur")
            .join(format!("{base}{WRITE_SEPARATOR}2,{info}"));
        fs::rename(&tmp, &target)?;
        Ok((target, notes))
    }

    /// Writes the `dovecot-keywords` files.
    pub fn finish(self) -> Result<()> {
        for (dir, keywords) in self.keywords {
            if keywords.is_empty() {
                continue;
            }
            let text: String =
                keywords
                    .iter()
                    .enumerate()
                    .fold(String::new(), |mut out, (i, k)| {
                        let _ = writeln!(out, "{i} {k}");
                        out
                    });
            let path = dir.join("dovecot-keywords");
            fs::write(&path, text)
                .map_err(|e| PortabilityError::io(path.display().to_string(), e))?;
        }
        Ok(())
    }
}

fn make_maildir(dir: &Path) -> Result<()> {
    for sub in ["cur", "new", "tmp"] {
        let path = dir.join(sub);
        fs::create_dir_all(&path)
            .map_err(|e| PortabilityError::io(path.display().to_string(), e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::portability::{JobKind, JobReport};
    use pretty_assertions::assert_eq;

    /// `cur` / `new` as junctions to another folder are not followed.
    #[cfg(windows)]
    #[test]
    fn junctioned_cur_and_new_are_not_followed() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "Subject: x\r\n\r\nsecret").unwrap();
        let md = dir.path().join("md");
        fs::create_dir(&md).unwrap();
        for sub in ["cur", "new"] {
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(md.join(sub))
                .arg(&outside)
                .output()
                .unwrap();
            assert!(status.status.success(), "{status:?}");
        }
        assert!(md.join("cur").is_dir(), "the junction works");
        assert!(!is_maildir(&md));
        assert_eq!(message_files(&md).len(), 0);
        assert!(scan(&md).is_err(), "no maildir: nothing to import");
        // A real maildir beside a junctioned one still works.
        fs::remove_dir(md.join("new")).unwrap();
        fs::create_dir(md.join("new")).unwrap();
        fs::write(md.join("new").join("1"), "Subject: y\r\n\r\nmine").unwrap();
        let folders = scan(&md).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].messages, 1);
    }

    fn ctx() -> JobContext<'static> {
        JobContext::silent(JobReport::started("j".into(), JobKind::Import, "t"))
    }

    fn read(folder: &MailFolderSource) -> Vec<RawMessage> {
        let FolderLocation::Maildir(dir) = &folder.location else {
            panic!("not a maildir");
        };
        let mut out = Vec::new();
        read_folder(dir, &folder.folder.path, &mut ctx(), &mut |_, m| {
            out.push(m);
            Ok(())
        })
        .unwrap();
        out
    }

    #[test]
    fn info_flags() {
        assert_eq!(split_info("123.abc:2,FS"), ("123.abc", Some("FS")));
        assert_eq!(split_info("123.abc;2,"), ("123.abc", Some("")));
        assert_eq!(split_info("123.abc!2,RS"), ("123.abc", Some("RS")));
        assert_eq!(split_info("123.abc"), ("123.abc", None));
        let info = parse_info("DFPRSTacX");
        assert!(info.flags.draft && info.flags.flagged && info.flags.answered && info.flags.seen);
        assert!(info.trashed);
        assert_eq!(info.keyword_letters, vec!['a', 'c']);
        assert_eq!(info.unknown, vec!['X']);
    }

    #[test]
    fn courier_lists() {
        let list =
            parse_courier_list("$Label1\nwork\nurgent\n\n1700000000.M1P1.host:0 2\nother:1\nbad\n");
        assert_eq!(list["1700000000.M1P1.host"], vec!["$Label1", "urgent"]);
        assert_eq!(list["other"], vec!["work"]);
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn scans_maildir_plus_plus_and_nested_trees() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        make_maildir(root).unwrap();
        make_maildir(&root.join(".Sent")).unwrap();
        make_maildir(&root.join(".Lists.Rust")).unwrap();
        make_maildir(&root.join(".Entw&APw-rfe")).unwrap();
        make_maildir(&root.join("Projects/Nail")).unwrap();
        fs::create_dir_all(root.join(".notamaildir")).unwrap();
        let folders = scan(root).unwrap();
        let paths: Vec<_> = folders.iter().map(|f| f.folder.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "Entwürfe",
                "Inbox",
                "Lists",
                "Lists/Rust",
                "Projects",
                "Projects/Nail",
                "Sent"
            ]
        );
        assert_eq!(folders[2].location, FolderLocation::Empty);
        assert_eq!(folders[1].folder.role, nail_api::domain::FolderRole::Inbox);
        let empty = tempfile::tempdir().unwrap();
        assert!(matches!(
            scan(empty.path()),
            Err(PortabilityError::InvalidSource(_))
        ));
    }

    #[test]
    fn reads_flags_keywords_and_times() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        make_maildir(root).unwrap();
        fs::write(root.join("dovecot-keywords"), "0 $Label1\n1 work\n").unwrap();
        // Courier's list file is called `:list`, which Windows cannot
        // create (its parser is tested on its own above).
        let courier = !cfg!(windows);
        if courier {
            fs::create_dir_all(root.join("courierimapkeywords")).unwrap();
            fs::write(
                root.join("courierimapkeywords/:list"),
                "todo\n\n1700000002.M2P2.host:0\n",
            )
            .unwrap();
        }
        fs::write(
            root.join("cur/1700000001.M1P1.host;2,RSab"),
            "Subject: one\n\n1\n",
        )
        .unwrap();
        fs::write(
            root.join("cur/1700000002.M2P2.host!2,FTz"),
            "Subject: two\n\n2\n",
        )
        .unwrap();
        fs::write(
            root.join("new/1700000003.M3P3.host"),
            "Subject: three\n\n3\n",
        )
        .unwrap();
        fs::write(root.join("new/.hidden"), "x").unwrap();
        fs::write(root.join("tmp/1700000004.M4P4.host"), "Subject: partial\n").unwrap();
        let folders = scan(root).unwrap();
        assert_eq!(folders[0].messages, 3);
        let messages = read(&folders[0]);
        assert_eq!(messages.len(), 3);
        assert!(messages[0].flags.seen && messages[0].flags.answered);
        assert_eq!(messages[0].keywords, vec!["$Label1", "work"]);
        assert_eq!(messages[0].received_at, Some(1_700_000_001));
        assert!(messages[1].flags.flagged && messages[1].deleted);
        if courier {
            assert_eq!(messages[1].keywords, vec!["todo"]);
        }
        assert_eq!(messages[1].warnings.len(), 1, "unmapped letter z");
        assert!(!messages[2].flags.seen);
        assert_eq!(messages[2].raw, b"Subject: three\n\n3\n");
    }

    #[test]
    fn writes_a_tree_that_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("out");
        let mut writer = MaildirWriter::create(&root).unwrap();
        let (inbox, _) = writer.folder(None).unwrap();
        let (sub, changed) = writer.folder(Some("Lists/Rüst v1.0")).unwrap();
        assert!(changed);
        let seen = Flags {
            seen: true,
            flagged: true,
            ..Flags::default()
        };
        writer
            .write(
                &inbox,
                b"Subject: a\n\nA\n",
                seen,
                &["work".into(), "$Label1".into()],
                Some(1_700_000_000),
            )
            .unwrap();
        writer
            .write(
                &sub,
                b"Subject: b\n\nB\n",
                Flags::default(),
                &["work".into()],
                None,
            )
            .unwrap();
        writer.finish().unwrap();
        assert!(root.join(".Lists.R&APw-st v1_0/maildirfolder").exists());
        let folders = scan(&root).unwrap();
        let paths: Vec<_> = folders.iter().map(|f| f.folder.path.clone()).collect();
        assert_eq!(paths, vec!["Inbox", "Lists", "Lists/Rüst v1_0"]);
        let inbox_messages = read(&folders[0]);
        assert_eq!(inbox_messages[0].flags, seen);
        assert_eq!(inbox_messages[0].keywords, vec!["work", "$Label1"]);
        assert_eq!(inbox_messages[0].received_at, Some(1_700_000_000));
        let sub_messages = read(&folders[2]);
        assert_eq!(sub_messages[0].keywords, vec!["work"]);
        assert_eq!(sub_messages[0].raw, b"Subject: b\n\nB\n");
    }

    #[test]
    fn more_than_26_keywords_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = MaildirWriter::create(dir.path()).unwrap();
        let (inbox, _) = writer.folder(None).unwrap();
        let keywords: Vec<String> = (0..28).map(|i| format!("k{i}")).collect();
        let (_, notes) = writer
            .write(&inbox, b"S: x\n\n", Flags::default(), &keywords, None)
            .unwrap();
        assert_eq!(notes.dropped_keywords, vec!["k26", "k27"]);
    }
}
