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
//! Customization (M1b WP24): every error of a file is reported at once
//! ([`parse_theme_all`], [`parse_layout_all`], [`parse_accounts_all`],
//! [`ConfigManager::file_errors`], [`ConfigUpdate::Errors`]); UI edits are
//! written back into `layout.kdl` without losing comments or formatting
//! ([`apply_layout_patches`], [`ConfigManager::update_layout`]); the merge
//! dialog compares and merges an incoming theme or layout
//! ([`preview_merge`], [`resolve_merge`], [`ConfigManager::apply_merge`])
//! with backups and revert ([`ConfigManager::revert`]); the inspector opens
//! a file at a line ([`ConfigManager::open_file_command`]). See ADR 0026.
//!
//! The file formats are documented for users in `userDocs/en/theme-reference.md`,
//! `userDocs/en/layout-reference.md` and `userDocs/en/accounts-reference.md`.

#![warn(missing_docs)]

mod accounts;
mod backup;
mod css;
mod customize;
mod error;
mod kdl_edit;
mod kdl_util;
mod layout;
mod manager;
mod merge;
mod open;
mod render;
pub mod rules;
mod schema;
mod selector;
mod theme;
mod widgets;
mod write_back;

use std::sync::OnceLock;

use nail_api::{ResolvedLayout, ResolvedTheme};

pub use accounts::{
    ACCOUNTS_FILE, AccountsDocument, MAX_ACCOUNT_ID_LEN, MAX_UNDO_SEND_SECS,
    MIN_POLL_INTERVAL_SECS, accounts_schema, derive_account_id, is_valid_account_id,
    is_valid_identity_pattern, load_accounts, parse_accounts, parse_accounts_all,
};
pub use backup::{BACKUP_DIR, KEEP as BACKUPS_KEPT};
pub use error::{ConfigError, ConfigErrorKind, issues, issues_event};
pub use kdl_util::Located;
pub use layout::{KeyDef, LayoutFile};
pub use manager::{ConfigKind, ConfigManager, ConfigUpdate, ConfigWatcher, DEFAULT_DEBOUNCE};
pub use merge::{MergeOutcome, preview_merge, resolve_merge};
pub use open::{EDITOR_VARS, configured_editor, editor_command};
pub use schema::{layout_schema, theme_schema};
pub use theme::{
    HtmlDarkModeSetting, IconDef, MessageDef, ModeSetting, PartDef, RawCssDef, ThemeFile, TokenDef,
};
pub use write_back::{EditError, apply_layout_patches};

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
/// messages and source locations (e.g. `"theme.kdl"`). Returns the first
/// error; [`parse_theme_all`] returns all of them.
pub fn parse_theme(text: &str, file: &str) -> Result<ResolvedTheme, ConfigError> {
    let merged = theme_file_merged(text, file).map_err(|mut errors| errors.swap_remove(0))?;
    merged.resolve()
}

/// Parses and resolves a theme file, reporting every error of the file
/// (never an empty list) in file order. Reading errors are reported first;
/// resolution (token references, cycles) runs only on a file that reads
/// cleanly, so one typo does not cause follow-up errors.
pub fn parse_theme_all(text: &str, file: &str) -> Result<ResolvedTheme, Vec<ConfigError>> {
    theme_file_merged(text, file).map_err(sorted)?.resolve_all()
}

/// Parses and resolves a layout file. `file` is the name used in error
/// messages and source locations (e.g. `"layout.kdl"`). Returns the first
/// error; [`parse_layout_all`] returns all of them.
pub fn parse_layout(text: &str, file: &str) -> Result<ResolvedLayout, ConfigError> {
    let merged = layout_file_merged(text, file).map_err(|mut errors| errors.swap_remove(0))?;
    merged.resolve()
}

/// Parses and resolves a layout file, reporting every error of the file
/// (never an empty list) in file order; see [`parse_theme_all`].
pub fn parse_layout_all(text: &str, file: &str) -> Result<ResolvedLayout, Vec<ConfigError>> {
    layout_file_merged(text, file)
        .map_err(sorted)?
        .resolve_all()
}

fn sorted(mut errors: Vec<ConfigError>) -> Vec<ConfigError> {
    kdl_util::sort_errors(&mut errors);
    errors
}

/// Reads a theme file and merges it onto the defaults when it extends them.
pub(crate) fn theme_file_merged(text: &str, file: &str) -> Result<ThemeFile, Vec<ConfigError>> {
    let parsed = ThemeFile::parse_all(text, file)?;
    Ok(if parsed.extends.is_some() {
        default_theme_file().clone().merge(parsed)
    } else {
        parsed
    })
}

/// Reads a layout file and merges it onto the defaults when it extends them.
pub(crate) fn layout_file_merged(text: &str, file: &str) -> Result<LayoutFile, Vec<ConfigError>> {
    let parsed = LayoutFile::parse_all(text, file)?;
    Ok(if parsed.extends.is_some() {
        default_layout_file().clone().merge(parsed)
    } else {
        parsed
    })
}
