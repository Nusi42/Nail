//! `.nailplugin` packages and plug-in files.
//!
//! A package is a zip archive (stored or deflate) with `manifest.json` at
//! the top, the bundled JavaScript the manifest's `main` names, assets and
//! the `.ftl` catalogs of `contributes.locales`. [`PluginPackage::read`]
//! checks everything before anything is installed:
//!
//! * at most [`MAX_PACKAGE_BYTES`] packed **and** unpacked (entries are
//!   read with a running budget, so a zip bomb stops at the limit whatever
//!   its headers claim), at most [`MAX_FILES`] entries;
//! * every path is a plain relative path ([`is_safe_path`]: no `..`, no
//!   absolute or drive paths, no `\`, no Windows device names), unique
//!   ignoring case; no symbolic links, no encrypted entries, no
//!   compression other than stored / deflate;
//! * `manifest.json` parses ([`PluginManifest::parse`]; its issues are
//!   reported, not raised) and the files it names exist.
//!
//! **Installed packages stay packed** ([`PackageStore`]): the approved
//! archive is kept as `<plugins-dir>/<id>.nailplugin` and read into memory
//! when the plug-in starts, after its SHA-256 is compared with the hash the
//! user approved. Nothing is ever unpacked to disk, so a package cannot
//! write outside its place however its paths look (ADR 0051).
//!
//! Dev plug-ins (`nail plugin dev`) are read from their folder
//! ([`PackageFiles::Folder`]); paths are checked the same way and must not
//! leave the folder through links.

use std::collections::{BTreeMap, HashSet};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nail_api::plugins::admin::{MAX_PACKAGE_BYTES, PACKAGE_EXTENSION};
use nail_api::plugins::manifest::is_package_path;
use nail_api::plugins::{PluginManifest, PluginManifestIssue, PluginManifestIssueKind};
use nail_api::{ApiError, ErrorCode, LocalizedMessage, PluginId};
use sha2::{Digest, Sha256};
use zip::CompressionMethod;

use crate::l10n;

/// The manifest's file name.
pub const MANIFEST_FILE: &str = "manifest.json";
/// Most entries (files and folders) in a package.
pub const MAX_FILES: usize = 1000;

/// What is wrong with a package or a plug-in folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackageErrorKind {
    /// Larger than [`MAX_PACKAGE_BYTES`] (packed or unpacked).
    TooLarge,
    /// Not a zip archive.
    NotAZip,
    /// More than [`MAX_FILES`] entries.
    TooManyFiles,
    /// A path that is not a plain relative path inside the package.
    UnsafePath,
    /// Two entries with the same path (ignoring case).
    DuplicatePath,
    /// A symbolic link.
    Link,
    /// An encrypted entry.
    Encrypted,
    /// A compression method other than stored / deflate.
    Compression,
    /// No `manifest.json` at the top.
    NoManifest,
    /// The archive, a file or the folder cannot be read.
    Unreadable,
    /// The package is not the one that was approved.
    HashMismatch,
    /// A file is missing.
    MissingFile,
}

impl PackageErrorKind {
    /// The Fluent id.
    pub fn message_id(self) -> &'static str {
        match self {
            Self::TooLarge => l10n::PACKAGE_TOO_LARGE,
            Self::NotAZip => l10n::PACKAGE_NOT_A_ZIP,
            Self::TooManyFiles => l10n::PACKAGE_TOO_MANY_FILES,
            Self::UnsafePath => l10n::PACKAGE_UNSAFE_PATH,
            Self::DuplicatePath => l10n::PACKAGE_DUPLICATE_PATH,
            Self::Link => l10n::PACKAGE_LINK,
            Self::Encrypted => l10n::PACKAGE_ENCRYPTED,
            Self::Compression => l10n::PACKAGE_COMPRESSION,
            Self::NoManifest => l10n::PACKAGE_NO_MANIFEST,
            Self::Unreadable => l10n::PACKAGE_UNREADABLE,
            Self::HashMismatch => l10n::PACKAGE_HASH_MISMATCH,
            Self::MissingFile => l10n::PACKAGE_MISSING_FILE,
        }
    }

    /// The API error class.
    pub fn code(self) -> ErrorCode {
        match self {
            Self::TooLarge => ErrorCode::TooLarge,
            Self::HashMismatch => ErrorCode::Conflict,
            Self::MissingFile => ErrorCode::NotFound,
            _ => ErrorCode::InvalidRequest,
        }
    }
}

/// A refused package or folder.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{kind:?}{}{}", .path.as_deref().map(|p| format!(" at `{p}`")).unwrap_or_default(), .detail.as_deref().map(|d| format!(": {d}")).unwrap_or_default())]
pub struct PackageError {
    /// What.
    pub kind: PackageErrorKind,
    /// The entry (as written in the archive, or the package path).
    pub path: Option<String>,
    /// More detail (English).
    pub detail: Option<String>,
}

impl PackageError {
    fn new(kind: PackageErrorKind) -> Self {
        Self {
            kind,
            path: None,
            detail: None,
        }
    }

    fn at(kind: PackageErrorKind, path: impl Into<String>) -> Self {
        Self {
            kind,
            path: Some(path.into()),
            detail: None,
        }
    }

    fn detail(mut self, detail: impl ToString) -> Self {
        self.detail = Some(detail.to_string());
        self
    }

    /// The user-facing text (`$path`, `$detail`).
    pub fn localized(&self) -> LocalizedMessage {
        LocalizedMessage::new(self.kind.message_id())
            .arg("path", self.path.clone().unwrap_or_default())
            .arg("detail", self.detail.clone().unwrap_or_default())
    }

    /// As an API error.
    pub fn to_api_error(&self) -> ApiError {
        ApiError::new(self.kind.code(), self.to_string()).with_localized(self.localized())
    }
}

impl From<PackageError> for ApiError {
    fn from(e: PackageError) -> Self {
        e.to_api_error()
    }
}

/// Whether `path` is safe as a file path inside a package on every OS:
/// [`is_package_path`] (relative, `/`-separated, no `.` / `..` parts, plain
/// characters) and no part that Windows treats specially (device names
/// such as `con` or `nul.js`, trailing dots).
pub fn is_safe_path(path: &str) -> bool {
    const DEVICES: &[&str] = &[
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    is_package_path(path)
        && path.split('/').all(|part| {
            let stem = part
                .split('.')
                .next()
                .unwrap_or_default()
                .to_ascii_lowercase();
            !DEVICES.contains(&stem.as_str()) && !part.ends_with('.')
        })
}

/// SHA-256 of `bytes`, lower-case hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The files of a plug-in: an archive in memory or a dev folder.
#[derive(Debug, Clone)]
pub enum PackageFiles {
    /// Unpacked in memory (installed packages).
    Memory(Arc<BTreeMap<String, Vec<u8>>>),
    /// A folder on disk (dev plug-ins; the canonical path).
    Folder(PathBuf),
}

impl PackageFiles {
    /// In-memory files (tests, embedders).
    pub fn from_map(files: BTreeMap<String, Vec<u8>>) -> Self {
        Self::Memory(Arc::new(files))
    }

    /// A file's bytes. `path` must be a safe package path
    /// ([`is_safe_path`]); folder reads must stay inside the folder (links
    /// pointing out are refused) and are limited to
    /// [`MAX_PACKAGE_BYTES`].
    pub fn read(&self, path: &str) -> Result<Vec<u8>, PackageError> {
        if !is_safe_path(path) {
            return Err(PackageError::at(PackageErrorKind::UnsafePath, path));
        }
        match self {
            Self::Memory(files) => files
                .get(path)
                .cloned()
                .ok_or_else(|| PackageError::at(PackageErrorKind::MissingFile, path)),
            Self::Folder(root) => {
                let joined = root.join(path);
                let real = match joined.canonicalize() {
                    Ok(real) => real,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        return Err(PackageError::at(PackageErrorKind::MissingFile, path));
                    }
                    Err(e) => {
                        return Err(PackageError::at(PackageErrorKind::Unreadable, path).detail(e));
                    }
                };
                if !real.starts_with(root) {
                    return Err(PackageError::at(PackageErrorKind::Link, path));
                }
                let meta = std::fs::metadata(&real)
                    .map_err(|e| PackageError::at(PackageErrorKind::Unreadable, path).detail(e))?;
                if !meta.is_file() {
                    return Err(PackageError::at(PackageErrorKind::MissingFile, path));
                }
                if meta.len() > MAX_PACKAGE_BYTES {
                    return Err(PackageError::at(PackageErrorKind::TooLarge, path));
                }
                std::fs::read(&real)
                    .map_err(|e| PackageError::at(PackageErrorKind::Unreadable, path).detail(e))
            }
        }
    }

    /// Whether a file exists.
    pub fn exists(&self, path: &str) -> bool {
        match self {
            Self::Memory(files) => files.contains_key(path),
            Self::Folder(_) => self.read(path).is_ok(),
        }
    }
}

/// Problems with the files a valid manifest names: `main` and the locale
/// files must exist (reported as manifest issues so the install dialog
/// lists them with the rest).
pub fn check_files(manifest: &PluginManifest, files: &PackageFiles) -> Vec<PluginManifestIssue> {
    let mut issues = Vec::new();
    let missing = |kind, path: String, file: &str| PluginManifestIssue {
        kind,
        path,
        detail: Some(file.to_owned()),
    };
    if !files.exists(&manifest.main) {
        issues.push(missing(
            PluginManifestIssueKind::InvalidMain,
            "main".into(),
            &manifest.main,
        ));
    }
    for (tag, file) in &manifest.contributes.locales {
        if !files.exists(file) {
            issues.push(missing(
                PluginManifestIssueKind::InvalidLocale,
                format!("contributes.locales.{tag}"),
                file,
            ));
        }
    }
    issues
}

/// A package that was read and checked.
#[derive(Debug, Clone)]
pub struct PluginPackage {
    /// The manifest, or its issues (missing files included).
    pub manifest: Result<PluginManifest, Vec<PluginManifestIssue>>,
    /// Its files.
    pub files: PackageFiles,
    /// SHA-256 of the archive (hex).
    pub sha256: String,
    /// Archive size (bytes).
    pub size: u64,
}

impl PluginPackage {
    /// Reads and checks an archive (see the module docs). Structural
    /// problems are errors; manifest problems are in
    /// [`PluginPackage::manifest`].
    pub fn read(bytes: &[u8]) -> Result<Self, PackageError> {
        use PackageErrorKind as K;
        let size = bytes.len() as u64;
        if size > MAX_PACKAGE_BYTES {
            return Err(PackageError::new(K::TooLarge));
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| PackageError::new(K::NotAZip).detail(e))?;
        if archive.len() > MAX_FILES {
            return Err(PackageError::new(K::TooManyFiles));
        }
        let mut seen = HashSet::new();
        let mut files = BTreeMap::new();
        let mut budget = MAX_PACKAGE_BYTES;
        for i in 0..archive.len() {
            // Metadata first, without decompressing or decrypting.
            let (name, is_dir) = {
                let entry = archive
                    .by_index_raw(i)
                    .map_err(|e| PackageError::new(K::Unreadable).detail(e))?;
                let name = entry.name().to_owned();
                let is_dir = name.ends_with('/');
                let plain = name.strip_suffix('/').unwrap_or(&name);
                if !is_safe_path(plain) || entry.enclosed_name().is_none() {
                    return Err(PackageError::at(K::UnsafePath, name));
                }
                if entry.is_symlink() {
                    return Err(PackageError::at(K::Link, name));
                }
                if entry.encrypted() {
                    return Err(PackageError::at(K::Encrypted, name));
                }
                if !matches!(
                    entry.compression(),
                    CompressionMethod::Stored | CompressionMethod::Deflated
                ) {
                    return Err(PackageError::at(K::Compression, name));
                }
                if !seen.insert(plain.to_ascii_lowercase()) {
                    return Err(PackageError::at(K::DuplicatePath, name));
                }
                (name, is_dir)
            };
            if is_dir {
                continue;
            }
            let entry = archive
                .by_index(i)
                .map_err(|e| PackageError::at(K::Unreadable, &name).detail(e))?;
            let mut data = Vec::new();
            entry
                .take(budget + 1)
                .read_to_end(&mut data)
                .map_err(|e| PackageError::at(K::Unreadable, &name).detail(e))?;
            let len = data.len() as u64;
            if len > budget {
                return Err(PackageError::at(K::TooLarge, name));
            }
            budget -= len;
            files.insert(name, data);
        }
        let Some(manifest_bytes) = files.get(MANIFEST_FILE) else {
            return Err(PackageError::new(K::NoManifest));
        };
        let files = PackageFiles::from_map(files.clone());
        let manifest = parse_with_files(manifest_bytes, &files);
        Ok(Self {
            manifest,
            files,
            sha256: sha256_hex(bytes),
            size,
        })
    }

    /// The manifest when it is valid.
    pub fn valid_manifest(&self) -> Option<&PluginManifest> {
        self.manifest.as_ref().ok()
    }

    /// The manifest's issues (empty when valid).
    pub fn issues(&self) -> &[PluginManifestIssue] {
        match &self.manifest {
            Ok(_) => &[],
            Err(issues) => issues,
        }
    }
}

fn parse_with_files(
    bytes: &[u8],
    files: &PackageFiles,
) -> Result<PluginManifest, Vec<PluginManifestIssue>> {
    let manifest = PluginManifest::parse(bytes)?;
    let issues = check_files(&manifest, files);
    if issues.is_empty() {
        Ok(manifest)
    } else {
        Err(issues)
    }
}

/// Reads a dev plug-in folder: its `manifest.json` (parsed and checked
/// against the files) and the folder as [`PackageFiles::Folder`].
pub fn load_folder(
    folder: &Path,
) -> Result<
    (
        Result<PluginManifest, Vec<PluginManifestIssue>>,
        PackageFiles,
    ),
    PackageError,
> {
    let root = folder.canonicalize().map_err(|e| {
        PackageError::at(PackageErrorKind::Unreadable, folder.display().to_string()).detail(e)
    })?;
    if !root.is_dir() {
        return Err(PackageError::at(
            PackageErrorKind::Unreadable,
            folder.display().to_string(),
        ));
    }
    let files = PackageFiles::Folder(root);
    let bytes = match files.read(MANIFEST_FILE) {
        Ok(bytes) => bytes,
        Err(e) if e.kind == PackageErrorKind::MissingFile => {
            return Err(PackageError::new(PackageErrorKind::NoManifest));
        }
        Err(e) => return Err(e),
    };
    Ok((parse_with_files(&bytes, &files), files))
}

/// Installed packages: `<dir>/<id>.nailplugin`, kept packed.
#[derive(Debug, Clone)]
pub struct PackageStore {
    dir: PathBuf,
}

impl PackageStore {
    /// Packages in `dir` (`<data-dir>/plugins`); created on first install.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where the package of `id` is kept.
    pub fn path(&self, id: &PluginId) -> PathBuf {
        self.dir.join(format!("{id}.{PACKAGE_EXTENSION}"))
    }

    /// Installs `bytes` when its hash is `expected_sha256` (the one the
    /// user approved) and the package is valid; replaces an earlier
    /// version atomically. Returns the checked package.
    pub fn install(
        &self,
        bytes: &[u8],
        expected_sha256: &str,
    ) -> Result<PluginPackage, PackageError> {
        if !sha256_hex(bytes).eq_ignore_ascii_case(expected_sha256) {
            return Err(PackageError::new(PackageErrorKind::HashMismatch));
        }
        let package = PluginPackage::read(bytes)?;
        let Some(manifest) = package.valid_manifest() else {
            let detail = package
                .issues()
                .iter()
                .map(|i| format!("{:?} at {}", i.kind, i.path))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(
                PackageError::at(PackageErrorKind::Unreadable, MANIFEST_FILE).detail(detail),
            );
        };
        let io = |e: std::io::Error| PackageError::new(PackageErrorKind::Unreadable).detail(e);
        std::fs::create_dir_all(&self.dir).map_err(io)?;
        let target = self.path(&manifest.id);
        let temp = self.dir.join(format!(
            ".{}-{:016x}.tmp",
            manifest.id,
            rand::random::<u64>()
        ));
        std::fs::write(&temp, bytes).map_err(io)?;
        if let Err(e) = std::fs::rename(&temp, &target) {
            let _ = std::fs::remove_file(&temp);
            return Err(io(e));
        }
        Ok(package)
    }

    /// Reads an installed package; with `expected_sha256` it must be
    /// exactly the approved one.
    pub fn load(
        &self,
        id: &PluginId,
        expected_sha256: Option<&str>,
    ) -> Result<PluginPackage, PackageError> {
        let path = self.path(id);
        let bytes = std::fs::read(&path).map_err(|e| {
            let kind = if e.kind() == std::io::ErrorKind::NotFound {
                PackageErrorKind::MissingFile
            } else {
                PackageErrorKind::Unreadable
            };
            PackageError::at(kind, path.display().to_string()).detail(e)
        })?;
        if let Some(expected) = expected_sha256
            && !sha256_hex(&bytes).eq_ignore_ascii_case(expected)
        {
            return Err(PackageError::new(PackageErrorKind::HashMismatch));
        }
        PluginPackage::read(&bytes)
    }

    /// Removes an installed package (no error when it is not there).
    pub fn remove(&self, id: &PluginId) -> std::io::Result<()> {
        match std::fs::remove_file(self.path(id)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// Builds a package archive from `(path, bytes)` pairs (deflate), for
/// tests and tools.
pub fn build_package<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> zip::result::ZipResult<Vec<u8>> {
    use std::io::Write;
    let mut buf = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated);
        for (path, bytes) in files {
            zip.start_file(path, options)?;
            zip.write_all(bytes)?;
        }
        zip.finish()?;
    }
    Ok(buf.into_inner())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    const MANIFEST: &str = r#"{"id":"hello","name":"Hello","version":"1.0.0","apiVersion":"1","main":"dist/index.js",
        "contributes":{"locales":{"en":"locales/en.ftl"}}}"#;

    fn good() -> Vec<u8> {
        build_package([
            ("manifest.json", MANIFEST.as_bytes()),
            ("dist/index.js", b"export default {}".as_slice()),
            ("locales/en.ftl", b"x = y".as_slice()),
            ("assets/logo.png", [0x89u8, b'P', b'N', b'G'].as_slice()),
        ])
        .unwrap()
    }

    fn zip_with(entries: &[(&str, &[u8])], options: zip::write::SimpleFileOptions) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            for (name, data) in entries {
                zip.start_file(*name, options).unwrap();
                zip.write_all(data).unwrap();
            }
            zip.finish().unwrap();
        }
        buf.into_inner()
    }

    fn deflate() -> zip::write::SimpleFileOptions {
        zip::write::SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
    }

    fn kind(bytes: &[u8]) -> PackageErrorKind {
        PluginPackage::read(bytes).unwrap_err().kind
    }

    #[test]
    fn reads_a_valid_package() {
        let bytes = good();
        let pkg = PluginPackage::read(&bytes).unwrap();
        let manifest = pkg.valid_manifest().unwrap();
        assert_eq!(manifest.id.as_str(), "hello");
        assert_eq!(pkg.sha256, sha256_hex(&bytes));
        assert_eq!(pkg.sha256.len(), 64);
        assert_eq!(pkg.size, bytes.len() as u64);
        assert_eq!(
            pkg.files.read("dist/index.js").unwrap(),
            b"export default {}"
        );
        assert_eq!(
            pkg.files.read("nope.js").unwrap_err().kind,
            PackageErrorKind::MissingFile
        );
        assert_eq!(
            pkg.files.read("../manifest.json").unwrap_err().kind,
            PackageErrorKind::UnsafePath
        );
    }

    #[test]
    fn refuses_path_traversal_and_odd_paths() {
        for bad in [
            "../evil.js",
            "dist/../../evil.js",
            "/etc/passwd",
            "C:/Windows/evil.js",
            "dist\\..\\evil.js",
            "./x.js",
            "dist//x.js",
            "con.js",
            "dist/NUL",
            "trailing.",
            "sp ace.js",
        ] {
            let bytes = zip_with(
                &[("manifest.json", MANIFEST.as_bytes()), (bad, b"x")],
                deflate(),
            );
            assert_eq!(kind(&bytes), PackageErrorKind::UnsafePath, "{bad}");
        }
    }

    #[test]
    fn refuses_duplicates_ignoring_case() {
        let bytes = zip_with(
            &[
                ("manifest.json", MANIFEST.as_bytes()),
                ("dist/index.js", b"a"),
                ("DIST/Index.js", b"b"),
            ],
            deflate(),
        );
        assert_eq!(kind(&bytes), PackageErrorKind::DuplicatePath);
    }

    #[test]
    fn refuses_symlinks() {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            zip.start_file("manifest.json", deflate()).unwrap();
            zip.write_all(MANIFEST.as_bytes()).unwrap();
            zip.add_symlink("dist/index.js", "/etc/passwd", deflate())
                .unwrap();
            zip.finish().unwrap();
        }
        assert_eq!(kind(&buf.into_inner()), PackageErrorKind::Link);
    }

    #[test]
    fn refuses_other_compression_methods() {
        let mut bytes = zip_with(&[("manifest.json", MANIFEST.as_bytes())], deflate());
        // Patch the method (offset 8 of the local header, 10 of the
        // central directory entry) to 12 (bzip2).
        let local = 0;
        bytes[local + 8] = 12;
        let central = bytes
            .windows(4)
            .position(|w| w == [0x50, 0x4b, 0x01, 0x02])
            .unwrap();
        bytes[central + 10] = 12;
        assert_eq!(kind(&bytes), PackageErrorKind::Compression);
    }

    #[test]
    fn refuses_encrypted_entries() {
        let mut bytes = zip_with(&[("manifest.json", MANIFEST.as_bytes())], deflate());
        // Set the "encrypted" bit of the general purpose flags.
        bytes[6] |= 1;
        let central = bytes
            .windows(4)
            .position(|w| w == [0x50, 0x4b, 0x01, 0x02])
            .unwrap();
        bytes[central + 8] |= 1;
        assert_eq!(kind(&bytes), PackageErrorKind::Encrypted);
    }

    #[test]
    fn stops_zip_bombs_at_the_limit() {
        // 21 MiB of zeros deflate to ~21 KiB.
        let zeros = vec![0u8; (MAX_PACKAGE_BYTES + 1024 * 1024) as usize];
        let bytes = zip_with(
            &[("manifest.json", MANIFEST.as_bytes()), ("bomb.bin", &zeros)],
            deflate(),
        );
        assert!(bytes.len() < 1024 * 1024);
        assert_eq!(kind(&bytes), PackageErrorKind::TooLarge);
    }

    #[test]
    fn refuses_oversized_archives_and_garbage() {
        let big = vec![0u8; (MAX_PACKAGE_BYTES + 1) as usize];
        assert_eq!(kind(&big), PackageErrorKind::TooLarge);
        assert_eq!(kind(b"not a zip at all"), PackageErrorKind::NotAZip);
        let many: Vec<(String, Vec<u8>)> = (0..=MAX_FILES)
            .map(|i| (format!("f{i}.txt"), Vec::new()))
            .collect();
        let entries: Vec<(&str, &[u8])> = many
            .iter()
            .map(|(n, d)| (n.as_str(), d.as_slice()))
            .collect();
        assert_eq!(
            kind(&zip_with(&entries, deflate())),
            PackageErrorKind::TooManyFiles
        );
    }

    #[test]
    fn needs_a_manifest_and_its_files() {
        let bytes = zip_with(&[("index.js", b"x")], deflate());
        assert_eq!(kind(&bytes), PackageErrorKind::NoManifest);
        let nested = zip_with(&[("hello/manifest.json", MANIFEST.as_bytes())], deflate());
        assert_eq!(kind(&nested), PackageErrorKind::NoManifest);

        let missing = zip_with(&[("manifest.json", MANIFEST.as_bytes())], deflate());
        let pkg = PluginPackage::read(&missing).unwrap();
        let kinds: Vec<_> = pkg.issues().iter().map(|i| i.kind).collect();
        assert_eq!(
            kinds,
            [
                PluginManifestIssueKind::InvalidMain,
                PluginManifestIssueKind::InvalidLocale
            ]
        );

        let invalid = zip_with(
            &[(
                "manifest.json",
                br#"{"id":"Bad","permissions":["mail.send"]}"#,
            )],
            deflate(),
        );
        let pkg = PluginPackage::read(&invalid).unwrap();
        assert!(pkg.valid_manifest().is_none());
        assert!(
            pkg.issues()
                .iter()
                .any(|i| i.kind == PluginManifestIssueKind::PermissionNotInPreview)
        );
    }

    #[test]
    fn errors_are_localized() {
        let err = PluginPackage::read(b"junk").unwrap_err();
        let api = err.to_api_error();
        assert_eq!(api.code, ErrorCode::InvalidRequest);
        assert_eq!(api.localized.unwrap().id, l10n::PACKAGE_NOT_A_ZIP);
        let err = PackageError::new(PackageErrorKind::HashMismatch);
        assert_eq!(err.to_api_error().code, ErrorCode::Conflict);
        assert_eq!(
            PackageError::new(PackageErrorKind::TooLarge)
                .to_api_error()
                .code,
            ErrorCode::TooLarge
        );
    }

    #[test]
    fn store_installs_loads_and_removes() {
        let dir = tempfile::tempdir().unwrap();
        let store = PackageStore::new(dir.path().join("plugins"));
        let bytes = good();
        let hash = sha256_hex(&bytes);
        assert_eq!(
            store.install(&bytes, &"0".repeat(64)).unwrap_err().kind,
            PackageErrorKind::HashMismatch
        );
        store.install(&bytes, &hash).unwrap();
        let id = PluginId::new("hello");
        assert!(store.path(&id).ends_with("hello.nailplugin"));
        let loaded = store.load(&id, Some(&hash)).unwrap();
        assert_eq!(loaded.sha256, hash);
        // Tampering after the approval is noticed.
        std::fs::write(store.path(&id), b"tampered").unwrap();
        assert_eq!(
            store.load(&id, Some(&hash)).unwrap_err().kind,
            PackageErrorKind::HashMismatch
        );
        store.remove(&id).unwrap();
        store.remove(&id).unwrap();
        assert_eq!(
            store.load(&id, None).unwrap_err().kind,
            PackageErrorKind::MissingFile
        );
        // Invalid manifests are not installed.
        let invalid = zip_with(&[("manifest.json", b"{}")], deflate());
        assert!(store.install(&invalid, &sha256_hex(&invalid)).is_err());
        // No temp files are left behind.
        let left: Vec<_> = std::fs::read_dir(store.dir()).unwrap().collect();
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn folders_are_read_safely() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("plugin");
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::create_dir_all(root.join("locales")).unwrap();
        std::fs::write(root.join("manifest.json"), MANIFEST).unwrap();
        std::fs::write(root.join("dist/index.js"), "export default {}").unwrap();
        std::fs::write(root.join("locales/en.ftl"), "x = y").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "secret").unwrap();
        let (manifest, files) = load_folder(&root).unwrap();
        assert_eq!(manifest.unwrap().id.as_str(), "hello");
        assert_eq!(files.read("dist/index.js").unwrap(), b"export default {}");
        assert_eq!(
            files.read("../secret.txt").unwrap_err().kind,
            PackageErrorKind::UnsafePath
        );
        assert_eq!(
            files.read("dist").unwrap_err().kind,
            PackageErrorKind::MissingFile
        );
        std::fs::remove_file(root.join("locales/en.ftl")).unwrap();
        let (manifest, _) = load_folder(&root).unwrap();
        assert_eq!(
            manifest.unwrap_err()[0].kind,
            PluginManifestIssueKind::InvalidLocale
        );
        assert_eq!(
            load_folder(dir.path()).unwrap_err().kind,
            PackageErrorKind::NoManifest
        );
        assert_eq!(
            load_folder(&dir.path().join("missing")).unwrap_err().kind,
            PackageErrorKind::Unreadable
        );
    }

    #[test]
    fn safe_paths() {
        for ok in [
            "manifest.json",
            "dist/index.js",
            "assets/a-b_c.png",
            "console.js",
        ] {
            assert!(is_safe_path(ok), "{ok}");
        }
        for bad in ["", "a/../b", "aux", "Com1.txt", "lpt9/x", "x./y"] {
            assert!(!is_safe_path(bad), "{bad}");
        }
    }
}
