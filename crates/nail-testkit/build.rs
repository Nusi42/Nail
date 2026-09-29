//! Embeds the mail corpus (`corpus/<folder>/*.eml` plus optional `flags.txt`)
//! into the crate, so `demo_corpus()` works without access to the source tree.
//! `corpus/crypto/` holds OpenPGP / S/MIME test vectors, not demo mail; it
//! is not embedded (tests read it from the source tree).

use std::fmt::Write as _;
use std::path::Path;
use std::{env, fs};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    let corpus = Path::new(&manifest_dir).join("corpus");
    println!("cargo::rerun-if-changed={}", corpus.display());

    let mut folders: Vec<_> = fs::read_dir(&corpus)
        .expect("corpus directory exists")
        .map(|entry| entry.expect("readable corpus entry").path())
        .filter(|path| path.is_dir() && !path.ends_with("crypto"))
        .collect();
    folders.sort();

    let mut out = String::from("&[\n");
    for folder in folders {
        println!("cargo::rerun-if-changed={}", folder.display());
        let name = folder.file_name().unwrap().to_string_lossy().into_owned();
        let flags_path = folder.join("flags.txt");
        let flags = if flags_path.exists() {
            format!("include_str!({:?})", flags_path.display().to_string())
        } else {
            "\"\"".to_owned()
        };
        let mut files: Vec<_> = fs::read_dir(&folder)
            .expect("readable corpus folder")
            .map(|entry| entry.expect("readable corpus entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "eml"))
            .collect();
        files.sort();
        writeln!(
            out,
            "    EmbeddedFolder {{ name: {name:?}, flags: {flags}, messages: &["
        )
        .unwrap();
        for file in files {
            let file_name = file.file_name().unwrap().to_string_lossy().into_owned();
            writeln!(
                out,
                "        ({file_name:?}, include_bytes!({:?})),",
                file.display().to_string()
            )
            .unwrap();
        }
        out.push_str("    ] },\n");
    }
    out.push_str("]\n");

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR is set by cargo");
    fs::write(Path::new(&out_dir).join("corpus.rs"), out).expect("write generated corpus.rs");
}
