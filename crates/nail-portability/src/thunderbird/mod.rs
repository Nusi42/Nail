//! Thunderbird profiles: finding them (`profiles.ini`), their local mail
//! (Local Folders and POP3 accounts: mbox files, `Name.sbd` subfolders,
//! `.msf` summaries ignored), address books (`abook.sqlite`,
//! `history.sqlite`, Mork `.mab`), accounts (`prefs.js`) and filters
//! (`msgFilterRules.dat`) as proposals.

pub mod abook;
pub mod accounts;
pub mod filters;
pub mod mork;
pub mod prefs;

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use nail_api::portability::{
    AccountProposal, FoundImportSource, ImportCollectionKind, ImportFormat, ImportSource,
    IssueSeverity, ItemIssue, ItemRef, PortabilityIssueCode, RuleProposal,
};

pub use accounts::mark_duplicates;

use crate::error::{PortabilityError, Result};
use crate::folder::guess_role;
use crate::import::{
    FolderLocation, MailFolderSource, PimCollectionSource, PimLocation, scan_mbox_file,
};
use crate::sink::SourceCollection;
use accounts::{ServerType, TbAccount};
use prefs::Prefs;

/// Deepest folder tree read.
pub use crate::folder::MAX_FOLDER_DEPTH as MAX_DEPTH;

/// What a profile holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileScan {
    /// Local mail folders (`<account name>/<folder>`), parents first.
    pub folders: Vec<MailFolderSource>,
    /// Address books.
    pub collections: Vec<PimCollectionSource>,
    /// Account proposals.
    pub accounts: Vec<AccountProposal>,
    /// Filter proposals.
    pub rules: Vec<RuleProposal>,
    /// Problems: accounts that cannot be proposed, unreadable files.
    pub issues: Vec<ItemIssue>,
}

/// Files in a Thunderbird mail folder that are not mbox folders.
fn is_mail_file_candidate(path: &Path) -> bool {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.starts_with('.') {
        return false;
    }
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
    {
        None => true,
        // Folder names may contain dots ("v1.0"); known non-mail
        // extensions are left out.
        Some(ext) => !matches!(
            ext.as_str(),
            "msf"
                | "dat"
                | "html"
                | "htm"
                | "json"
                | "sqlite"
                | "log"
                | "js"
                | "txt"
                | "bak"
                | "tmp"
                | "ini"
                | "mozlz4"
                | "sqlite-wal"
                | "sqlite-shm"
                | "lock"
                | "sbd"
        ),
    }
}

/// Whether a file is an mbox (empty, or a `From ` line first).
fn is_mbox_file(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 512];
    let Ok(n) = file.read(&mut head) else {
        return false;
    };
    let head = &head[..n];
    let start = head
        .iter()
        .position(|b| !matches!(b, b'\r' | b'\n'))
        .unwrap_or(head.len());
    head[start..].is_empty() || head[start..].starts_with(b"From ")
}

/// The local mail folders of one Thunderbird mail directory, named below
/// `top` (the account's name).
pub fn scan_mail_dir(dir: &Path, top: &str) -> Result<Vec<MailFolderSource>> {
    let mut out = Vec::new();
    out.push(MailFolderSource {
        folder: crate::folder::source_folder(&[top.to_owned()]).0,
        renamed_from: None,
        location: FolderLocation::Empty,
        messages: 0,
        size_bytes: 0,
        estimated: false,
    });
    scan_sbd(dir, &mut vec![top.to_owned()], 0, &mut out)?;
    // The account's own folders get roles by their names.
    for folder in &mut out {
        let segments: Vec<&str> = folder.folder.path.split('/').collect();
        if segments.len() == 2 {
            folder.folder.role = guess_role(segments[1]);
        }
    }
    Ok(out)
}

fn scan_sbd(
    dir: &Path,
    segments: &mut Vec<String>,
    depth: usize,
    out: &mut Vec<MailFolderSource>,
) -> Result<()> {
    let entries = crate::eml::sorted_entries(dir)?;
    let mut seen: HashSet<String> = HashSet::new();
    for (path, file_type) in &entries {
        if !file_type.is_file() || !is_mail_file_candidate(path) || !is_mbox_file(path) {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        segments.push(name.clone());
        out.push(scan_mbox_file(path, segments)?);
        let sbd = dir.join(format!("{name}.sbd"));
        if sbd.is_dir() && depth < MAX_DEPTH {
            scan_sbd(&sbd, segments, depth + 1, out)?;
        }
        segments.pop();
        seen.insert(name);
    }
    // `.sbd` folders without an mbox of their own.
    for (path, file_type) in &entries {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(base) = name.strip_suffix(".sbd") else {
            continue;
        };
        if !file_type.is_dir() || seen.contains(base) || depth >= MAX_DEPTH {
            continue;
        }
        segments.push(base.to_owned());
        let (folder, renamed) = crate::folder::source_folder(segments);
        out.push(MailFolderSource {
            folder,
            renamed_from: renamed.then(|| segments.join("/")),
            location: FolderLocation::Empty,
            messages: 0,
            size_bytes: 0,
            estimated: false,
        });
        scan_sbd(path, segments, depth + 1, out)?;
        segments.pop();
    }
    Ok(())
}

/// Resolves `directory-rel` (`[ProfD]Mail/Local Folders`) or `directory`.
fn server_dir(profile: &Path, prefs: &Prefs, server_key: &str) -> Option<PathBuf> {
    if let Some(rel) = prefs.text(&format!("mail.server.{server_key}.directory-rel"))
        && let Some(rest) = rel.strip_prefix("[ProfD]")
    {
        let mut path = profile.to_path_buf();
        for part in rest.split(['/', '\\']).filter(|p| !p.is_empty()) {
            path.push(part);
        }
        return Some(path);
    }
    prefs
        .text(&format!("mail.server.{server_key}.directory"))
        .map(PathBuf::from)
}

fn issue(
    code: PortabilityIssueCode,
    severity: IssueSeverity,
    file: &Path,
    detail: impl Into<String>,
) -> ItemIssue {
    ItemIssue::new(
        code,
        severity,
        ItemRef {
            file: Some(file.display().to_string()),
            ..ItemRef::default()
        },
        detail,
    )
}

/// Scans a profile folder.
pub fn scan_profile(profile: &Path) -> Result<ProfileScan> {
    let prefs_path = profile.join("prefs.js");
    if !prefs_path.is_file() {
        return Err(PortabilityError::InvalidSource(format!(
            "{} is not a Thunderbird profile (no prefs.js)",
            profile.display()
        )));
    }
    let text = fs::read(&prefs_path)
        .map_err(|e| PortabilityError::io(prefs_path.display().to_string(), e))?;
    let prefs = Prefs::parse(&String::from_utf8_lossy(&text));
    let mut out = ProfileScan::default();
    let all = accounts::accounts(&prefs);
    let local_name = all
        .iter()
        .find(|a| a.server_type == ServerType::None)
        .map_or_else(|| "Local Folders".to_owned(), |a| a.name.clone());

    // Accounts.
    for account in &all {
        match accounts::proposal(&prefs, account) {
            Ok(Some(proposal)) => out.accounts.push(proposal),
            Ok(None) => {}
            Err(issue) => out.issues.push(*issue),
        }
    }

    // Local mail and filters per server.
    let mut mail_dirs: Vec<(PathBuf, String)> = Vec::new();
    for account in &all {
        let dir = server_dir(profile, &prefs, &account.server_key);
        if account.server_type.has_local_mail()
            && let Some(dir) = dir.clone().filter(|d| d.is_dir())
        {
            mail_dirs.push((dir, account.name.clone()));
        }
        if let Some(dir) = dir {
            read_filters(&dir, account, &prefs, &local_name, &mut out);
        }
    }
    if mail_dirs.is_empty() {
        // Profiles without account prefs: the default Local Folders.
        let default = profile.join("Mail").join("Local Folders");
        if default.is_dir() {
            mail_dirs.push((default, local_name.clone()));
        }
    }
    let mut used_tops: HashSet<String> = HashSet::new();
    for (dir, name) in mail_dirs {
        let mut top = name.replace('/', "_");
        while !used_tops.insert(top.clone()) {
            top.push('_');
        }
        out.folders.extend(scan_mail_dir(&dir, &top)?);
    }

    // Address books.
    read_address_books(profile, &prefs, &mut out);
    Ok(out)
}

fn read_filters(
    dir: &Path,
    account: &TbAccount,
    prefs: &Prefs,
    local_name: &str,
    out: &mut ProfileScan,
) {
    let path = dir.join("msgFilterRules.dat");
    let Ok(bytes) = fs::read(&path) else {
        return;
    };
    let local = account.server_type == ServerType::None;
    let ctx = filters::FilterContext {
        account_key: (!local).then(|| account.key.clone()),
        local_folders: local,
        host: account.host.clone(),
        local_folders_name: local_name.to_owned(),
        periodic_minutes: prefs
            .int(&format!(
                "mail.server.{}.periodicFilterRateMinutes",
                account.server_key
            ))
            .or_else(|| prefs.int("mail.server.default.periodicFilterRateMinutes"))
            .and_then(|m| u32::try_from(m).ok())
            .unwrap_or(10),
    };
    let text = String::from_utf8_lossy(&bytes);
    for (index, filter) in filters::parse_file(&text).iter().enumerate() {
        out.rules.push(filters::convert(filter, index, &ctx));
    }
}

/// Address books: `ldap_2.servers.*` with a `filename` (SQLite or Mork),
/// plus `abook.sqlite` / `history.sqlite` when not listed.
fn read_address_books(profile: &Path, prefs: &Prefs, out: &mut ProfileScan) {
    let mut files: Vec<(String, Option<String>)> = Vec::new();
    for name in prefs.names_with_prefix("ldap_2.servers.") {
        let Some(server) = name
            .strip_prefix("ldap_2.servers.")
            .and_then(|n| n.strip_suffix(".filename"))
        else {
            continue;
        };
        if let Some(file) = prefs.text(name) {
            let description = prefs
                .text(&format!("ldap_2.servers.{server}.description"))
                .filter(|d| !d.starts_with("chrome://"));
            files.push((file, description));
        }
    }
    for default in ["abook.sqlite", "history.sqlite", "abook.mab", "history.mab"] {
        if !files.iter().any(|(f, _)| f == default) {
            files.push((default.to_owned(), None));
        }
    }
    let mut seen = HashSet::new();
    for (file, description) in files {
        if file.contains(['/', '\\']) || !seen.insert(file.clone()) {
            continue;
        }
        let path = profile.join(&file);
        if !path.is_file() {
            continue;
        }
        // A Mork book that was migrated to SQLite is not read twice.
        if let Some(stem) = file.strip_suffix(".mab")
            && profile.join(format!("{stem}.sqlite")).is_file()
        {
            continue;
        }
        let location = if Path::new(&file)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("mab"))
        {
            PimLocation::Mork(path.clone())
        } else {
            PimLocation::AddressBookSqlite(path.clone())
        };
        match location.read() {
            Ok(nail_ical::import::ImportData::Cards(cards)) => {
                let stem = Path::new(&file)
                    .file_stem()
                    .map_or_else(|| file.clone(), |s| s.to_string_lossy().into_owned());
                out.collections.push(PimCollectionSource {
                    collection: SourceCollection {
                        key: file.clone(),
                        name: description.unwrap_or(stem),
                        kind: ImportCollectionKind::AddressBook,
                    },
                    items: (cards.contacts.len() + cards.groups.len()) as u64,
                    location,
                });
            }
            Ok(_) => {}
            Err(e) => out.issues.push(issue(
                PortabilityIssueCode::UnsupportedFormat,
                IssueSeverity::Skipped,
                &path,
                e.to_string(),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Finding profiles

/// The Thunderbird data folder of the current user
/// (`%APPDATA%\Thunderbird` on Windows), if `APPDATA` / `HOME` is set.
pub fn default_data_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Thunderbird"))
    } else {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".thunderbird"))
    }
}

/// The profiles listed in `<data_dir>/profiles.ini` that exist, the
/// default first (from `installs.ini` / an `[Install…]` section, else
/// `Default=1`).
pub fn find_profiles(data_dir: &Path) -> Vec<FoundImportSource> {
    let Ok(text) = fs::read_to_string(data_dir.join("profiles.ini")) else {
        return Vec::new();
    };
    let sections = parse_ini(&text);
    let installs = fs::read_to_string(data_dir.join("installs.ini"))
        .map(|t| parse_ini(&t))
        .unwrap_or_default();
    let install_default: Option<String> = sections
        .iter()
        .filter(|(name, _)| name.starts_with("Install"))
        .chain(installs.iter())
        .find_map(|(_, values)| value(values, "Default").map(str::to_owned));
    let mut out = Vec::new();
    for (name, values) in &sections {
        if !name.starts_with("Profile") {
            continue;
        }
        let Some(path) = value(values, "Path") else {
            continue;
        };
        let relative = value(values, "IsRelative") != Some("0");
        let full = if relative {
            let mut p = data_dir.to_path_buf();
            for part in path.split(['/', '\\']).filter(|s| !s.is_empty()) {
                p.push(part);
            }
            p
        } else {
            PathBuf::from(path)
        };
        if !full.join("prefs.js").is_file() {
            continue;
        }
        let is_default = match &install_default {
            Some(default) => default.replace('\\', "/") == path.replace('\\', "/"),
            None => value(values, "Default") == Some("1"),
        };
        let last_used_at = fs::metadata(full.join("prefs.js"))
            .and_then(|m| m.modified())
            .ok()
            .and_then(nail_util::time::unix_secs_of)
            .map(nail_util::time::rfc3339_utc);
        out.push(FoundImportSource {
            source: ImportSource {
                format: ImportFormat::ThunderbirdProfile,
                path: full.display().to_string(),
            },
            label: value(values, "Name").unwrap_or(path).to_owned(),
            is_default,
            last_used_at,
        });
    }
    out.sort_by_key(|f| !f.is_default);
    out
}

type IniSection = (String, Vec<(String, String)>);

fn parse_ini(text: &str) -> Vec<IniSection> {
    let mut out: Vec<IniSection> = Vec::new();
    for line in text.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() || line.starts_with([';', '#']) {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.push((name.to_owned(), Vec::new()));
        } else if let Some((key, value)) = line.split_once('=')
            && let Some((_, values)) = out.last_mut()
        {
            values.push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
    out
}

fn value<'a>(values: &'a [(String, String)], key: &str) -> Option<&'a str> {
    values
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_profiles_default_first() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path();
        for p in ["Profiles/a.default", "Profiles/b.work"] {
            fs::create_dir_all(data.join(p)).unwrap();
            fs::write(data.join(p).join("prefs.js"), "").unwrap();
        }
        fs::write(
            data.join("profiles.ini"),
            "[General]\nStartWithLastProfile=1\n\n[Profile0]\nName=default\nIsRelative=1\nPath=Profiles/a.default\nDefault=1\n\n[Profile1]\nName=work\nIsRelative=1\nPath=Profiles/b.work\n\n[Profile2]\nName=gone\nIsRelative=1\nPath=Profiles/missing\n\n[Install308046B0AF4A39CB]\nDefault=Profiles/b.work\nLocked=1\n",
        )
        .unwrap();
        let found = find_profiles(data);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].label, "work");
        assert!(found[0].is_default);
        assert!(!found[1].is_default);
        assert_eq!(found[0].source.format, ImportFormat::ThunderbirdProfile);
        assert!(found[0].source.path.ends_with("b.work"));
        assert!(found[0].last_used_at.is_some());
        assert_eq!(find_profiles(&data.join("nothing")).len(), 0);
    }

    #[test]
    fn candidates() {
        assert!(is_mail_file_candidate(Path::new("Inbox")));
        assert!(is_mail_file_candidate(Path::new("Release v1.0")));
        assert!(!is_mail_file_candidate(Path::new("Inbox.msf")));
        assert!(!is_mail_file_candidate(Path::new("msgFilterRules.dat")));
        assert!(!is_mail_file_candidate(Path::new(".DS_Store")));
    }

    #[test]
    fn a_folder_without_prefs_is_not_a_profile() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            scan_profile(dir.path()),
            Err(PortabilityError::InvalidSource(_))
        ));
    }
}
