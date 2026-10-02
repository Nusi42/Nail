//! Builds the SQLite3 Multiple Ciphers amalgamation with the compile
//! options of `libsqlite3-sys` 0.38.2's bundled build (so behaviour stays
//! the same for plain databases) plus the options encryption needs, and
//! copies the pre-generated bindings of SQLite 3.53.2 (the amalgamation is
//! 3.53.4: a patch release with the same API).

use std::env;
use std::path::Path;

#[cfg(any(
    feature = "sqlcipher",
    feature = "bundled-sqlcipher",
    feature = "bundled-sqlcipher-vendored-openssl"
))]
compile_error!("Nail's libsqlite3-sys bundles SQLite3 Multiple Ciphers; SQLCipher is not available (ADR 0095)");

#[cfg(any(feature = "buildtime_bindgen", feature = "loadable_extension", feature = "preupdate_hook", feature = "session", feature = "in_gecko"))]
compile_error!("this feature of libsqlite3-sys is not supported by Nail's SQLite3 Multiple Ciphers build (ADR 0095)");

fn main() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR");
    std::fs::copy(
        "bindings/bindgen_bundled_version.rs",
        Path::new(&out_dir).join("bindgen.rs"),
    )
    .expect("copy the bindings");
    println!("cargo:rerun-if-changed=bindings/bindgen_bundled_version.rs");
    println!("cargo:rerun-if-changed=build.rs");

    let src = sqlite3mc_src::source_dir();
    let windows = env::var("CARGO_CFG_WINDOWS").is_ok();
    let msvc = env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|v| v == "msvc");

    let mut cfg = cc::Build::new();
    cfg.file(src.join(sqlite3mc_src::SOURCE_FILE))
        // The options of libsqlite3-sys's bundled build.
        .define("SQLITE_CORE", None)
        .define("SQLITE_DEFAULT_FOREIGN_KEYS", "1")
        .define("SQLITE_ENABLE_API_ARMOR", None)
        .define("SQLITE_ENABLE_COLUMN_METADATA", None)
        .define("SQLITE_ENABLE_DBSTAT_VTAB", None)
        .define("SQLITE_ENABLE_FTS3", None)
        .define("SQLITE_ENABLE_FTS3_PARENTHESIS", None)
        .define("SQLITE_ENABLE_FTS5", None)
        .define("SQLITE_ENABLE_JSON1", None)
        .define("SQLITE_ENABLE_LOAD_EXTENSION", "1")
        .define("SQLITE_ENABLE_MEMORY_MANAGEMENT", None)
        .define("SQLITE_ENABLE_RTREE", None)
        .define("SQLITE_ENABLE_STAT4", None)
        .define("SQLITE_SOUNDEX", None)
        .define("SQLITE_THREADSAFE", "1")
        .define("SQLITE_USE_URI", None)
        .define("HAVE_USLEEP", "1")
        .define("HAVE_ISNAN", None)
        .define("_POSIX_THREAD_SAFE_FUNCTIONS", None)
        // Encryption: temporary tables and indices stay in memory, so no
        // plaintext temp file is written next to an encrypted database.
        .define("SQLITE_TEMP_STORE", "2")
        .warnings(false);
    if !windows {
        cfg.define("HAVE_LOCALTIME_R", None);
    }
    if env::var("CARGO_CFG_TARGET_FEATURE")
        .is_ok_and(|v| v.split(',').any(|tf| tf == "crt-static"))
        && msvc
    {
        cfg.static_crt(true);
    }
    if cfg!(feature = "unlock_notify") {
        cfg.define("SQLITE_ENABLE_UNLOCK_NOTIFY", None);
    }
    for name in ["SQLITE_MAX_VARIABLE_NUMBER", "SQLITE_MAX_EXPR_DEPTH", "SQLITE_MAX_COLUMN"] {
        println!("cargo:rerun-if-env-changed={name}");
        if let Ok(limit) = env::var(name) {
            cfg.define(name, limit.as_str());
        }
    }
    cfg.compile("sqlite3");
    println!("cargo:include={}", src.display());
    println!("cargo:lib_dir={out_dir}");
}
