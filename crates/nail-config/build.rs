//! Embeds the Fluent catalogs (`locales/<lang>/*.ftl`) into the crate: the
//! directories are the single list of languages and catalog files, so a new
//! file or language needs no line anywhere (wave Q2, ADR 0171). Writes
//! `$OUT_DIR/catalogs.rs` (`LANGUAGES` and `CATALOGS`), included by
//! `src/l10n.rs`.

use std::error::Error;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

/// The fallback language: complete, listed first.
const ENGLISH: &str = "en";

/// `(name, path)` of the entries of `dir` that `keep` accepts (by file
/// type and name), sorted by name.
fn listing(
    dir: &Path,
    keep: impl Fn(&fs::FileType, &str) -> bool,
) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if keep(&entry.file_type()?, &name) {
            found.push((name, entry.path().display().to_string()));
        }
    }
    found.sort();
    Ok(found)
}

fn main() -> Result<(), Box<dyn Error>> {
    let locales = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("../../locales");
    println!("cargo::rerun-if-changed={}", locales.display());
    let mut languages = listing(&locales, |kind, _| kind.is_dir())?;
    // English first: the fallback of every other language.
    languages.sort_by_key(|(lang, _)| lang != ENGLISH);

    let mut names = Vec::new();
    let mut catalogs = String::new();
    for (lang, dir) in languages {
        println!("cargo::rerun-if-changed={dir}");
        writeln!(catalogs, "    ({lang:?}, &[")?;
        let files = listing(Path::new(&dir), |kind, name| {
            kind.is_file() && Path::new(name).extension().is_some_and(|e| e == "ftl")
        })?;
        for (file, path) in files {
            writeln!(catalogs, "        ({file:?}, include_str!({path:?})),")?;
        }
        writeln!(catalogs, "    ]),")?;
        names.push(format!("{lang:?}"));
    }
    let out = format!(
        "/// The languages with embedded catalogs (the folders of `locales/`),\n\
         /// English first.\n\
         pub const LANGUAGES: &[&str] = &[{}];\n\n\
         /// The catalogs of each language of [`LANGUAGES`], in that order: file\n\
         /// name and text, sorted by name (English is complete; other languages\n\
         /// may be empty, English fills the gaps per key).\n\
         pub const CATALOGS: &[(&str, &[(&str, &str)])] = &[\n{catalogs}];\n",
        names.join(", ")
    );
    fs::write(PathBuf::from(env::var("OUT_DIR")?).join("catalogs.rs"), out)?;
    Ok(())
}
