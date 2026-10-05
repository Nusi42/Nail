//! Embeds the mail corpus (`corpus/<folder>/*.eml` plus optional `flags.txt`)
//! and the P2 demo conversations (`conversations/<folder>/…`, the same
//! layout) into the crate, so `demo_corpus()` and
//! `demo_corpus_with_conversations()` work without access to the source tree.
//! `corpus/crypto/` holds OpenPGP / S/MIME test vectors, not demo mail; it
//! is not embedded (tests read it from the source tree).

use std::error::Error;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

/// The entries of `dir` that pass `keep`, sorted.
fn sorted_entries(
    dir: &Path,
    keep: impl Fn(&Path) -> bool,
) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("read {}: {e}", dir.display()))? {
        let path = entry?.path();
        if keep(&path) {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

/// The last component of `path`, as text.
fn file_name(path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(path
        .file_name()
        .ok_or_else(|| format!("{} has no file name", path.display()))?
        .to_string_lossy()
        .into_owned())
}

/// The Rust source of an `&[EmbeddedFolder]` for the corpus directory `dir`.
fn embed_dir(dir: &Path) -> Result<String, Box<dyn Error>> {
    println!("cargo::rerun-if-changed={}", dir.display());
    let folders = sorted_entries(dir, |path| path.is_dir() && !path.ends_with("crypto"))?;

    let mut out = String::from(
        "&[
",
    );
    for folder in folders {
        println!("cargo::rerun-if-changed={}", folder.display());
        let name = file_name(&folder)?;
        let flags_path = folder.join("flags.txt");
        let flags = if flags_path.exists() {
            format!("include_str!({:?})", flags_path.display().to_string())
        } else {
            "\"\"".to_owned()
        };
        let files = sorted_entries(&folder, |path| {
            path.extension().is_some_and(|ext| ext == "eml")
        })?;
        writeln!(
            out,
            "    EmbeddedFolder {{ name: {name:?}, flags: {flags}, messages: &["
        )?;
        for file in files {
            let file_name = file_name(&file)?;
            writeln!(
                out,
                "        ({file_name:?}, include_bytes!({:?})),",
                file.display().to_string()
            )?;
        }
        out.push_str(
            "    ] },
",
        );
    }
    out.push_str(
        "]
",
    );
    Ok(out)
}

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    let out_dir = env::var("OUT_DIR")?;
    // `corpus/`: the demo corpus; `conversations/`: the P2 demo
    // conversations the seeds of `nail serve` add to it.
    for (dir, target) in [
        ("corpus", "corpus.rs"),
        ("conversations", "conversations.rs"),
    ] {
        let source = embed_dir(&Path::new(&manifest_dir).join(dir))?;
        fs::write(Path::new(&out_dir).join(target), source)?;
    }
    Ok(())
}
