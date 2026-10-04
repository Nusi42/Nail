//! The diagnostics bundle (contract `nail_api::diagnostics`): redaction
//! and the zip writer. The core collects the content (logs, configuration,
//! statistics, chosen messages); this module redacts it, builds the
//! manifest the preview shows and writes exactly that.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::Path;

use nail_api::diagnostics::{
    DIAGNOSTICS_MANIFEST_ENTRY, DIAGNOSTICS_PREVIEW_BYTES, DiagnosticsBundle, DiagnosticsEntry,
    DiagnosticsItem, DiagnosticsManifest, DiagnosticsOptions, Redaction,
};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::backup::valid_entry_name;
use crate::error::{PortabilityError, Result};

/// One file the core collected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectedEntry {
    /// Its name in the bundle (`logs/nail.log`).
    pub name: String,
    /// The item it belongs to.
    pub item: DiagnosticsItem,
    /// The content.
    pub content: Vec<u8>,
}

/// An entry after redaction, ready to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedEntry {
    /// The manifest's view of it.
    pub entry: DiagnosticsEntry,
    /// The bytes written.
    pub content: Vec<u8>,
}

/// Replaces secrets always, and mail addresses, subjects / display names
/// and the user's name in paths when asked. Placeholders are stable within
/// one bundle (the same address is always `user1@example.invalid`).
#[derive(Debug, Clone, Default)]
pub struct Redactor {
    /// Replace addresses and subjects.
    pub addresses: bool,
    /// The user's name as it appears in paths (`sam` in `C:\Users\sam`).
    pub user_name: Option<String>,
    placeholders: HashMap<String, String>,
}

const SECRET_KEYS: [&str; 12] = [
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "access_token",
    "refresh_token",
    "client_secret",
    "api_key",
    "apikey",
    "authorization",
    "pin",
];

/// The placeholder of a removed value.
pub const REDACTED: &str = "<redacted>";

impl Redactor {
    /// A redactor for `options`; `user_name` is the account name in the
    /// user's home folder path.
    pub fn new(options: &DiagnosticsOptions, user_name: Option<String>) -> Self {
        Self {
            addresses: options.redact_addresses,
            user_name: user_name.filter(|u| u.len() >= 2),
            placeholders: HashMap::new(),
        }
    }

    /// Redacts a text; returns it and the redactions applied.
    pub fn redact(&mut self, text: &str) -> (String, Vec<Redaction>) {
        let mut applied = vec![Redaction::Secrets];
        let mut out = redact_secrets(text);
        if self.addresses {
            out = self.redact_addresses(&out);
            out = redact_subjects(&out);
            applied.push(Redaction::Addresses);
            applied.push(Redaction::Subjects);
        }
        if let Some(user) = &self.user_name {
            out = redact_user_paths(&out, user);
            applied.push(Redaction::Paths);
        }
        (out, applied)
    }

    fn redact_addresses(&mut self, text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let is_local = |c: char| c.is_alphanumeric() || "._%+-'".contains(c);
        let is_domain = |c: char| c.is_alphanumeric() || ".-".contains(c);
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        let mut copied = 0;
        while i < chars.len() {
            if chars[i] == '@' {
                let mut start = i;
                while start > copied && is_local(chars[start - 1]) {
                    start -= 1;
                }
                let mut end = i + 1;
                while end < chars.len() && is_domain(chars[end]) {
                    end += 1;
                }
                // Trailing dots are punctuation.
                while end > i + 1 && chars[end - 1] == '.' {
                    end -= 1;
                }
                let domain: String = chars[i + 1..end].iter().collect();
                if start < i && domain.contains('.') && !domain.starts_with('.') {
                    let address: String =
                        chars[start..end].iter().collect::<String>().to_lowercase();
                    let n = self.placeholders.len() + 1;
                    let placeholder = self
                        .placeholders
                        .entry(address)
                        .or_insert_with(|| format!("user{n}@example.invalid"))
                        .clone();
                    out.extend(&chars[copied..start]);
                    out.push_str(&placeholder);
                    copied = end;
                    i = end;
                    continue;
                }
            }
            i += 1;
        }
        out.extend(&chars[copied..]);
        out
    }
}

/// Removes values of secret-looking keys (`password=…`, `"token": "…"`,
/// `Authorization: Bearer …`), `keychain:` references and PEM private
/// keys.
pub fn redact_secrets(text: &str) -> String {
    let mut lines = Vec::new();
    let mut in_pem = false;
    for line in text.split_inclusive('\n') {
        if line.contains("-----BEGIN") && line.contains("PRIVATE KEY") {
            in_pem = true;
            lines.push(format!("{REDACTED}{}", line_end(line)));
            continue;
        }
        if in_pem {
            if line.contains("-----END") {
                in_pem = false;
            }
            continue;
        }
        lines.push(redact_secret_line(line));
    }
    lines.concat()
}

fn line_end(line: &str) -> &str {
    if line.ends_with("\r\n") {
        "\r\n"
    } else if line.ends_with('\n') {
        "\n"
    } else {
        ""
    }
}

fn redact_secret_line(line: &str) -> String {
    let mut out = line.to_owned();
    // `keychain:` references.
    let mut search = 0;
    while let Some(at) = out[search..].find("keychain:") {
        let start = search + at + "keychain:".len();
        let end = out[start..]
            .find(|c: char| c.is_whitespace() || "\"',;)>]}".contains(c))
            .map_or(out.len(), |e| start + e);
        out.replace_range(start..end, REDACTED);
        search = start + REDACTED.len();
    }
    // Bearer / Basic credentials.
    for scheme in ["Bearer ", "bearer ", "Basic ", "basic "] {
        let mut search = 0;
        while let Some(at) = out[search..].find(scheme) {
            let start = search + at + scheme.len();
            let end = out[start..]
                .find(|c: char| c.is_whitespace() || "\"',;".contains(c))
                .map_or(out.len(), |e| start + e);
            if end > start {
                out.replace_range(start..end, REDACTED);
            }
            search = start + REDACTED.len().min(out.len() - start);
        }
    }
    // key=value, key: value, "key": "value".
    let lower = out.to_ascii_lowercase();
    let mut replacements: Vec<(usize, usize)> = Vec::new();
    for key in SECRET_KEYS {
        let mut search = 0;
        while let Some(at) = lower[search..].find(key) {
            let key_start = search + at;
            let key_end = key_start + key.len();
            search = key_end;
            // A whole word: not part of a longer identifier.
            let before = lower[..key_start].chars().next_back();
            let after = lower[key_end..].chars().next();
            if before.is_some_and(|c| c.is_ascii_alphanumeric())
                || after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                continue;
            }
            let rest = &out[key_end..];
            let trimmed = rest.trim_start_matches(['"', '\'']).trim_start();
            let Some(sep) = trimmed.chars().next().filter(|c| *c == '=' || *c == ':') else {
                continue;
            };
            let value_start = key_end + (rest.len() - trimmed.len()) + sep.len_utf8();
            let value_text = &out[value_start..];
            let skip_ws = value_text.len() - value_text.trim_start().len();
            let value_start = value_start + skip_ws;
            let value_text = &out[value_start..];
            let (start, end) = if let Some(quote) = value_text
                .chars()
                .next()
                .filter(|c| *c == '"' || *c == '\'')
            {
                let close = value_text[1..]
                    .find(quote)
                    .map_or(value_text.len(), |e| e + 1);
                (value_start + 1, value_start + close)
            } else {
                let end = value_text
                    .find(|c: char| c.is_whitespace() || ",;}&".contains(c))
                    .unwrap_or(value_text.len());
                (value_start, value_start + end)
            };
            // Already redacted, or nothing.
            if end > start && &out[start..end] != REDACTED && !out[start..end].starts_with("Bearer")
            {
                replacements.push((start, end));
            }
        }
    }
    replacements.sort_unstable();
    replacements.dedup();
    let mut result = String::with_capacity(out.len());
    let mut copied = 0;
    for (start, end) in replacements {
        if start < copied {
            continue;
        }
        result.push_str(&out[copied..start]);
        result.push_str(REDACTED);
        copied = end;
    }
    result.push_str(&out[copied..]);
    result
}

/// Removes subjects (`Subject: …` lines, `subject="…"` fields) and display
/// names before addresses (`Ada Lovelace <…>`).
pub fn redact_subjects(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if let Some(at) = lower.find("subject") {
                let after = &line[at + "subject".len()..];
                let trimmed = after.trim_start_matches(['"', '\'']).trim_start();
                if trimmed.starts_with(['=', ':']) {
                    let value_start = at + "subject".len() + (after.len() - trimmed.len()) + 1;
                    let value = line[value_start..].trim_start();
                    let value_start = line.len() - value.len();
                    // A quoted value ends at its quote, else at the line end.
                    if let Some(quote) = value.chars().next().filter(|c| *c == '"' || *c == '\'')
                        && let Some(close) = value[1..].find(quote)
                    {
                        let end = value_start + 1 + close;
                        return format!(
                            "{}{REDACTED}{}",
                            &line[..=value_start],
                            redact_display_names(&line[end..])
                        );
                    }
                    return format!(
                        "{} {REDACTED}{}",
                        line[..value_start].trim_end(),
                        line_end(line)
                    );
                }
            }
            redact_display_names(line)
        })
        .collect()
}

fn redact_display_names(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(lt) = rest.find('<') {
        let Some(gt) = rest[lt..].find('>') else {
            break;
        };
        let inside = &rest[lt + 1..lt + gt];
        if inside.contains('@') {
            let before = &rest[..lt];
            // The name runs back to a separator.
            let name_start = before.rfind([',', ':', '(', '[', '=']).map_or(0, |i| i + 1);
            let name = before[name_start..].trim();
            if !name.is_empty() {
                out.push_str(&before[..name_start]);
                out.push(' ');
                out.push_str(REDACTED);
                out.push(' ');
                out.push_str(&rest[lt..=(lt + gt)]);
                rest = &rest[lt + gt + 1..];
                continue;
            }
        }
        out.push_str(&rest[..=(lt + gt)]);
        rest = &rest[lt + gt + 1..];
    }
    out.push_str(rest);
    out
}

/// Replaces the user's name inside home folder paths
/// (`C:\Users\sam\…`, `/home/sam/…`, `C:/Users/sam`).
pub fn redact_user_paths(text: &str, user: &str) -> String {
    let mut out = text.to_owned();
    for prefix in ["Users\\", "Users/", "users\\", "users/", "home/", "HOME/"] {
        let needle = format!("{prefix}{user}");
        let mut search = 0;
        while let Some(at) = out[search..]
            .to_ascii_lowercase()
            .find(&needle.to_ascii_lowercase())
        {
            let start = search + at + prefix.len();
            let end = start + user.len();
            let next = out[end..].chars().next();
            if next.is_none_or(|c| c == '\\' || c == '/' || !c.is_alphanumeric()) {
                out.replace_range(start..end, "<user>");
                search = start + "<user>".len();
            } else {
                search = end;
            }
        }
    }
    out
}

/// The first [`DIAGNOSTICS_PREVIEW_BYTES`] of a text, cut at a character.
fn preview_of(text: &str) -> String {
    if text.len() <= DIAGNOSTICS_PREVIEW_BYTES {
        return text.to_owned();
    }
    let mut end = DIAGNOSTICS_PREVIEW_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// Redacts the collected entries and builds the manifest
/// ([`nail_api::Request::PreviewDiagnostics`] shows it; the bundle writes
/// exactly these entries). Entries of items not chosen are dropped; mail
/// items are kept as the user chose them (not redacted).
pub fn prepare(
    collected: Vec<CollectedEntry>,
    options: &DiagnosticsOptions,
    redactor: &mut Redactor,
    created_at: &str,
    app_version: &str,
) -> Result<(DiagnosticsManifest, Vec<PreparedEntry>)> {
    let problems = options.issues();
    if !problems.is_empty() {
        return Err(PortabilityError::InvalidOptions(
            problems
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    let mut prepared: Vec<PreparedEntry> = Vec::new();
    for entry in collected {
        if !options.items.contains(&entry.item) {
            continue;
        }
        if !valid_entry_name(&entry.name) || entry.name == DIAGNOSTICS_MANIFEST_ENTRY {
            return Err(PortabilityError::InvalidOptions(format!(
                "invalid entry name {:?}",
                entry.name
            )));
        }
        if prepared.iter().any(|p| p.entry.name == entry.name) {
            return Err(PortabilityError::InvalidOptions(format!(
                "entry {} twice",
                entry.name
            )));
        }
        let (content, redactions, preview) = match String::from_utf8(entry.content) {
            Ok(text) if entry.item.is_mail() => {
                let preview = preview_of(&text);
                (text.into_bytes(), Vec::new(), Some(preview))
            }
            Ok(text) => {
                let (redacted, redactions) = redactor.redact(&text);
                let preview = preview_of(&redacted);
                (redacted.into_bytes(), redactions, Some(preview))
            }
            // Binary content cannot be redacted: only mail items may hold it.
            Err(e) if entry.item.is_mail() => (e.into_bytes(), Vec::new(), None),
            Err(_) => {
                return Err(PortabilityError::InvalidOptions(format!(
                    "{} is not text and cannot be redacted",
                    entry.name
                )));
            }
        };
        prepared.push(PreparedEntry {
            entry: DiagnosticsEntry {
                name: entry.name,
                item: entry.item,
                size_bytes: content.len() as u64,
                redactions,
                preview,
            },
            content,
        });
    }
    let manifest = DiagnosticsManifest {
        created_at: created_at.to_owned(),
        app_version: app_version.to_owned(),
        options: options.clone(),
        entries: prepared.iter().map(|p| p.entry.clone()).collect(),
    };
    Ok((manifest, prepared))
}

/// Writes the bundle ([`nail_api::Request::CreateDiagnosticsBundle`]):
/// `manifest.json` and exactly the prepared entries.
pub fn write_bundle(
    path: &Path,
    manifest: &DiagnosticsManifest,
    entries: &[PreparedEntry],
    overwrite: bool,
) -> Result<DiagnosticsBundle> {
    crate::files::check_target(path, overwrite)?;
    let io = |e: std::io::Error| PortabilityError::io(path.display().to_string(), e);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io)?;
    }
    // Written under a temporary name and renamed into place: a confirmed
    // overwrite replaces the old bundle atomically.
    let temp = crate::files::temp_path(path);
    if let Err(err) = write_zip(&temp, manifest, entries) {
        drop(fs::remove_file(&temp));
        return Err(err);
    }
    crate::files::persist(&temp, path, overwrite)?;
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut file = File::open(path).map_err(io)?;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        size += n as u64;
    }
    Ok(DiagnosticsBundle {
        path: path.display().to_string(),
        manifest: manifest.clone(),
        size_bytes: size,
        sha256: hex::encode(hasher.finalize()),
    })
}

/// Writes the bundle's zip to `temp` (synced).
fn write_zip(temp: &Path, manifest: &DiagnosticsManifest, entries: &[PreparedEntry]) -> Result<()> {
    let io = |e: std::io::Error| PortabilityError::io(temp.display().to_string(), e);
    let zip_error = |e: zip::result::ZipError| {
        PortabilityError::io(temp.display().to_string(), std::io::Error::other(e))
    };
    let file = File::create(temp).map_err(io)?;
    let mut zip = ZipWriter::new(BufWriter::new(file));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file(DIAGNOSTICS_MANIFEST_ENTRY, options)
        .map_err(zip_error)?;
    serde_json::to_writer_pretty(&mut zip, manifest).map_err(|e| io(e.into()))?;
    for prepared in entries {
        zip.start_file(prepared.entry.name.as_str(), options)
            .map_err(zip_error)?;
        zip.write_all(&prepared.content).map_err(io)?;
    }
    let buffered = zip.finish().map_err(zip_error)?;
    let file = buffered.into_inner().map_err(|e| io(e.into_error()))?;
    file.sync_all().map_err(io)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::ids::MessageId;
    use pretty_assertions::assert_eq;

    #[test]
    fn secrets_are_always_removed() {
        let text = "connect user=ada password=hunter2 ok\n\
                    {\"refresh_token\": \"1//abc\", \"name\": \"x\"}\n\
                    Authorization: Bearer eyJhbGciOi\n\
                    account \"work\" { password \"keychain:nail/work/password\" }\n\
                    passwordless=true pin: 1234\n\
                    -----BEGIN PRIVATE KEY-----\nMIIEv\nabc\n-----END PRIVATE KEY-----\nafter\n";
        let out = redact_secrets(text);
        assert!(!out.contains("hunter2"), "{out}");
        assert!(!out.contains("1//abc"), "{out}");
        assert!(!out.contains("eyJhbGciOi"), "{out}");
        assert!(!out.contains("nail/work/password"), "{out}");
        assert!(!out.contains("1234"), "{out}");
        assert!(!out.contains("MIIEv"), "{out}");
        assert!(out.contains("user=ada"));
        assert!(out.contains("passwordless=true"), "not a secret key: {out}");
        assert!(out.contains("\"name\": \"x\""));
        assert!(out.ends_with("after\n"));
    }

    #[test]
    fn addresses_subjects_and_paths() {
        let options = DiagnosticsOptions::default();
        let mut redactor = Redactor::new(&options, Some("sam".into()));
        let (out, applied) = redactor.redact(
            "from Ada Lovelace <Ada@Example.org> to bob@example.org.\n\
             again ada@example.org\n\
             Subject: Secret plans\n\
             log subject=\"Hi\" path C:\\Users\\sam\\AppData and /home/sam/x and C:\\Users\\samother\n",
        );
        assert_eq!(
            applied,
            vec![
                Redaction::Secrets,
                Redaction::Addresses,
                Redaction::Subjects,
                Redaction::Paths
            ]
        );
        assert!(!out.to_lowercase().contains("ada@example.org"), "{out}");
        assert!(!out.contains("Ada Lovelace"), "{out}");
        assert!(out.contains("<user1@example.invalid>"), "{out}");
        assert!(out.contains("to user2@example.invalid."), "{out}");
        assert!(out.contains("again user1@example.invalid"), "stable: {out}");
        assert!(!out.contains("Secret plans"));
        assert!(!out.contains("\"Hi\""));
        assert!(out.contains("C:\\Users\\<user>\\AppData"));
        assert!(out.contains("/home/<user>/x"));
        assert!(out.contains("samother"), "another user's folder stays");
    }

    #[test]
    fn without_address_redaction() {
        let options = DiagnosticsOptions {
            redact_addresses: false,
            ..DiagnosticsOptions::default()
        };
        let mut redactor = Redactor::new(&options, None);
        let (out, applied) = redactor.redact("ada@example.org password=x\n");
        assert_eq!(applied, vec![Redaction::Secrets]);
        assert_eq!(out, "ada@example.org password=<redacted>\n");
    }

    fn collected() -> Vec<CollectedEntry> {
        vec![
            CollectedEntry {
                name: "logs/nail.log".into(),
                item: DiagnosticsItem::Logs,
                content: format!("start\npassword=x by ada@example.org\n{}", "y".repeat(5000))
                    .into_bytes(),
            },
            CollectedEntry {
                name: "config/accounts.kdl".into(),
                item: DiagnosticsItem::Config,
                content: b"account \"work\" email=\"ada@example.org\"\n".to_vec(),
            },
            CollectedEntry {
                name: "mail/m1.eml".into(),
                item: DiagnosticsItem::MailContent,
                content: b"From: ada@example.org\r\n\r\nbody\r\n".to_vec(),
            },
        ]
    }

    #[test]
    fn the_bundle_holds_exactly_the_preview() {
        let dir = tempfile::tempdir().unwrap();
        let options = DiagnosticsOptions::default();
        let mut redactor = Redactor::new(&options, None);
        let (manifest, prepared) = prepare(
            collected(),
            &options,
            &mut redactor,
            "2026-10-02T10:00:00Z",
            "1.0.0-rc.1",
        )
        .unwrap();
        // The mail entry is not chosen by default.
        assert_eq!(manifest.entries.len(), 2);
        let log = &manifest.entries[0];
        assert_eq!(
            log.preview.as_ref().unwrap().len(),
            DIAGNOSTICS_PREVIEW_BYTES
        );
        assert!(!log.preview.as_ref().unwrap().contains("ada@"));
        assert!(log.redactions.contains(&Redaction::Secrets));
        let path = dir.path().join("diag.zip");
        let bundle = write_bundle(&path, &manifest, &prepared, false).unwrap();
        assert_eq!(bundle.size_bytes, fs::metadata(&path).unwrap().len());
        assert_eq!(bundle.sha256.len(), 64);
        let mut archive = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
        let names: Vec<String> = archive.file_names().map(str::to_owned).collect();
        assert_eq!(
            names,
            vec!["manifest.json", "logs/nail.log", "config/accounts.kdl"]
        );
        let mut log_text = String::new();
        archive
            .by_name("logs/nail.log")
            .unwrap()
            .read_to_string(&mut log_text)
            .unwrap();
        assert_eq!(log_text.len() as u64, log.size_bytes);
        assert!(!log_text.contains("password=x"));
        let mut manifest_text = String::new();
        archive
            .by_name("manifest.json")
            .unwrap()
            .read_to_string(&mut manifest_text)
            .unwrap();
        let back: DiagnosticsManifest = serde_json::from_str(&manifest_text).unwrap();
        assert_eq!(back, manifest);
        assert!(matches!(
            write_bundle(&path, &manifest, &prepared, false),
            Err(PortabilityError::TargetExists(_))
        ));
        // Post-M4c: a confirmed overwrite replaces the file (through a
        // temporary file that is gone afterwards). Windows renames only
        // over files nobody holds open.
        drop(archive);
        fs::write(&path, b"old").unwrap();
        let again = write_bundle(&path, &manifest, &prepared, true).unwrap();
        assert_eq!(again.sha256, bundle.sha256);
        assert_eq!(again.size_bytes, fs::metadata(&path).unwrap().len());
        assert!(!crate::files::temp_path(&path).exists());
    }

    #[test]
    fn chosen_mail_is_kept_as_is() {
        let options = DiagnosticsOptions {
            items: vec![DiagnosticsItem::MailContent],
            message_ids: vec![MessageId::new("m1")],
            redact_addresses: true,
        };
        let mut redactor = Redactor::new(&options, None);
        let (manifest, prepared) = prepare(collected(), &options, &mut redactor, "t", "v").unwrap();
        assert_eq!(manifest.entries.len(), 1);
        assert!(manifest.entries[0].redactions.is_empty());
        assert_eq!(
            prepared[0].content,
            b"From: ada@example.org\r\n\r\nbody\r\n"
        );
    }

    #[test]
    fn invalid_options_and_entries() {
        let options = DiagnosticsOptions {
            items: vec![],
            ..DiagnosticsOptions::default()
        };
        let mut redactor = Redactor::new(&options, None);
        assert!(prepare(vec![], &options, &mut redactor, "t", "v").is_err());
        let options = DiagnosticsOptions::default();
        let bad = vec![CollectedEntry {
            name: "../escape".into(),
            item: DiagnosticsItem::Logs,
            content: vec![],
        }];
        assert!(prepare(bad, &options, &mut redactor, "t", "v").is_err());
        let binary = vec![CollectedEntry {
            name: "logs/x".into(),
            item: DiagnosticsItem::Logs,
            content: vec![0xff, 0xfe, 0x00],
        }];
        assert!(prepare(binary, &options, &mut redactor, "t", "v").is_err());
    }
}
