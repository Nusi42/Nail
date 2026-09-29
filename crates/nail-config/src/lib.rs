//! Theme and layout configuration.
//!
//! Parses `theme.kdl` and `layout.kdl` (KDL v2) into the resolved models of
//! [`nail_api::config`]:
//!
//! 1. The file is read into a file model ([`ThemeFile`], [`LayoutFile`]),
//!    checking every node, property and value; errors carry file, line and
//!    column ([`ConfigError`]).
//! 2. A sparse file with `extends="default"` is merged onto the shipped
//!    defaults (`defaults/theme.kdl`, `defaults/layout.kdl`).
//! 3. The merged model is resolved: token references such as `$color.accent`
//!    become `var(--nail-color-accent)`, selectors are parsed, cross
//!    references (tokens, toolbars) are checked.
//!
//! [`ConfigManager`] loads a config directory, keeps the last valid
//! configuration and watches the files for changes. [`theme_schema`] and
//! [`layout_schema`] export JSON Schemas of both files.
//!
//! `accounts.kdl` (accounts, identities, virtual accounts) is handled by
//! [`parse_accounts`] and [`AccountsDocument`], which also writes the file
//! back without losing comments or formatting; [`accounts_schema`] is its
//! JSON Schema. It is loaded by the core's account manager, not by
//! [`ConfigManager`].
//!
//! `rules.kdl` (filter rules) is parsed by [`rules`] (M1b WP22).
//!
//! The file formats are documented for users in `userDocs/en/theme-reference.md`,
//! `userDocs/en/layout-reference.md` and `userDocs/en/accounts-reference.md`.

#![warn(missing_docs)]

mod accounts;
mod css;
mod error;
mod kdl_util;
mod layout;
mod manager;
pub mod rules;
mod schema;
mod selector;
mod theme;
mod widgets;

use std::sync::OnceLock;

use nail_api::{ResolvedLayout, ResolvedTheme};

pub use accounts::{
    ACCOUNTS_FILE, AccountsDocument, MAX_ACCOUNT_ID_LEN, MAX_UNDO_SEND_SECS,
    MIN_POLL_INTERVAL_SECS, accounts_schema, derive_account_id, is_valid_account_id,
    is_valid_identity_pattern, load_accounts, parse_accounts,
};
pub use error::{ConfigError, ConfigErrorKind};
pub use kdl_util::Located;
pub use layout::{KeyDef, LayoutFile};
pub use manager::{ConfigKind, ConfigManager, ConfigUpdate, ConfigWatcher, DEFAULT_DEBOUNCE};
pub use schema::{layout_schema, theme_schema};
pub use theme::{
    HtmlDarkModeSetting, IconDef, MessageDef, ModeSetting, PartDef, RawCssDef, ThemeFile, TokenDef,
};

/// File name of the user's theme in the config directory.
pub const THEME_FILE: &str = "theme.kdl";
/// File name of the user's layout in the config directory.
pub const LAYOUT_FILE: &str = "layout.kdl";

/// Display name of the shipped default theme in error messages and sources.
pub const DEFAULT_THEME_SOURCE: &str = "defaults/theme.kdl";
/// Display name of the shipped default layout in error messages and sources.
pub const DEFAULT_LAYOUT_SOURCE: &str = "defaults/layout.kdl";

/// The shipped default theme (`defaults/theme.kdl`).
pub const DEFAULT_THEME_KDL: &str = include_str!("../../../defaults/theme.kdl");
/// The shipped default layout (`defaults/layout.kdl`).
pub const DEFAULT_LAYOUT_KDL: &str = include_str!("../../../defaults/layout.kdl");

fn default_theme_file() -> &'static ThemeFile {
    static FILE: OnceLock<ThemeFile> = OnceLock::new();
    FILE.get_or_init(|| {
        ThemeFile::parse(DEFAULT_THEME_KDL, DEFAULT_THEME_SOURCE)
            .unwrap_or_else(|e| panic!("the shipped default theme is invalid: {e}"))
    })
}

fn default_layout_file() -> &'static LayoutFile {
    static FILE: OnceLock<LayoutFile> = OnceLock::new();
    FILE.get_or_init(|| {
        LayoutFile::parse(DEFAULT_LAYOUT_KDL, DEFAULT_LAYOUT_SOURCE)
            .unwrap_or_else(|e| panic!("the shipped default layout is invalid: {e}"))
    })
}

/// The resolved default theme.
pub fn default_theme() -> ResolvedTheme {
    static THEME: OnceLock<ResolvedTheme> = OnceLock::new();
    THEME
        .get_or_init(|| {
            default_theme_file()
                .resolve()
                .unwrap_or_else(|e| panic!("the shipped default theme is invalid: {e}"))
        })
        .clone()
}

/// The resolved default layout.
pub fn default_layout() -> ResolvedLayout {
    static LAYOUT: OnceLock<ResolvedLayout> = OnceLock::new();
    LAYOUT
        .get_or_init(|| {
            default_layout_file()
                .resolve()
                .unwrap_or_else(|e| panic!("the shipped default layout is invalid: {e}"))
        })
        .clone()
}

/// Parses and resolves a theme file. `file` is the name used in error
/// messages and source locations (e.g. `"theme.kdl"`).
pub fn parse_theme(text: &str, file: &str) -> Result<ResolvedTheme, ConfigError> {
    let parsed = ThemeFile::parse(text, file)?;
    let merged = if parsed.extends.is_some() {
        default_theme_file().clone().merge(parsed)
    } else {
        parsed
    };
    merged.resolve()
}

/// Parses and resolves a layout file. `file` is the name used in error
/// messages and source locations (e.g. `"layout.kdl"`).
pub fn parse_layout(text: &str, file: &str) -> Result<ResolvedLayout, ConfigError> {
    let parsed = LayoutFile::parse(text, file)?;
    let merged = if parsed.extends.is_some() {
        default_layout_file().clone().merge(parsed)
    } else {
        parsed
    };
    merged.resolve()
}
