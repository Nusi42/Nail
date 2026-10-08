//! `accounts.kdl`: accounts, identities, virtual accounts, OAuth2
//! providers and (M3a, ADR 0058) calendar / contact sources:
//! `calendar-subscription`, `caldav` and `carddav` nodes.
//!
//! * [`parse_accounts`] reads and validates a file into
//!   [`nail_api::AccountsConfig`], with the same compiler-like errors
//!   ([`ConfigError`]) as the theme and layout files.
//! * [`AccountsDocument`] is the file as the UI edits it: add, update and
//!   remove accounts and virtual accounts, then [`AccountsDocument::save`].
//!   Edits keep comments, blank lines, indentation and everything they do
//!   not touch.
//! * [`accounts_schema`] is the JSON Schema of the file.
//!
//! Secrets never appear in the file, only keychain references
//! (`secret="keychain:nail/work/password"`). The format is documented for
//! users in `userDocs/en/accounts-reference.md` and decided in
//! `docs/adr/0013-accounts-file.md`.

mod read;
mod schema;
pub(crate) mod write;

use std::path::Path;

use nail_api::accounts::{AccountConfig, AccountsConfig, VirtualAccountConfig};
use nail_api::ids::{MAX_CONFIG_ID_LEN, first_free_id};
use nail_api::pim::PimSourceConfig;
use nail_api::{AccountId, IdentityId, PimSourceId};

use crate::error::{ConfigError, ConfigErrorKind};
use crate::kdl_util::Source;

pub use schema::accounts_schema;

/// File name of the account configuration in the config directory.
pub const ACCOUNTS_FILE: &str = "accounts.kdl";

/// Shortest accepted poll interval (`poll-interval`) in seconds.
pub const MIN_POLL_INTERVAL_SECS: u32 = 30;

/// Longest accepted `undo-send-delay` in seconds.
pub const MAX_UNDO_SEND_SECS: u32 = 600;

/// Parses and validates `accounts.kdl`. `file` is the name used in error
/// messages (e.g. `"accounts.kdl"`). An empty file is a valid, empty
/// configuration.
pub fn parse_accounts(text: &str, file: &str) -> Result<AccountsConfig, ConfigError> {
    let src = Source::new(file, text);
    read::read(&src)
}

/// Like [`parse_accounts`], but reports every error of the file (never an
/// empty list), in file order.
pub fn parse_accounts_all(text: &str, file: &str) -> Result<AccountsConfig, Vec<ConfigError>> {
    let src = Source::new(file, text);
    read::read_all(&src)
}

/// Loads `<dir>/accounts.kdl`; a missing file is an empty configuration.
pub fn load_accounts(dir: &Path) -> Result<AccountsConfig, ConfigError> {
    AccountsDocument::load(dir).map(|doc| doc.config)
}

/// Whether `id` is a valid account or virtual account name: 1 to 64
/// characters from `a-z 0-9 - _ .`, starting with a letter or digit. (The
/// reserved name [`AccountId::ALL`] is valid here but refused by the file.)
pub fn is_valid_account_id(id: &str) -> bool {
    nail_api::ids::is_config_id(id)
}

/// Whether `pattern` is a valid identity-rule pattern: `*@domain`,
/// `*@*.domain` or a full address.
pub fn is_valid_identity_pattern(pattern: &str) -> bool {
    let domain_ok = |d: &str| {
        !d.is_empty()
            && !d.starts_with('.')
            && !d.ends_with('.')
            && !d.contains(['*', '@'])
            && !d.chars().any(|c| c.is_whitespace() || c.is_control())
    };
    if let Some(domain) = pattern.strip_prefix("*@*.") {
        domain_ok(domain)
    } else if let Some(domain) = pattern.strip_prefix("*@") {
        domain_ok(domain)
    } else {
        !pattern.contains('*') && read::is_plausible_email(pattern)
    }
}

/// Derives a free account id from `hint` (an email address or a display
/// name): lowercased, other characters turned into `-`, shortened, and
/// suffixed with `-2`, `-3`, … when `taken` (or reserved).
pub fn derive_account_id(hint: &str, taken: impl Fn(&str) -> bool) -> AccountId {
    let mut slug = String::new();
    for c in hint.trim().chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches(['-', '.', '_']).to_owned();
    let mut base: String = slug.chars().take(MAX_CONFIG_ID_LEN - 4).collect();
    base.truncate(base.trim_end_matches(['-', '.', '_']).len());
    if base.is_empty() {
        "account".clone_into(&mut base);
    }
    AccountId::new(first_free_id(&base, |id| id == AccountId::ALL || taken(id)))
}

/// `accounts.kdl` as a document the UI edits.
///
/// Holds the file text and its parsed configuration. Every edit computes
/// the new text with the format-preserving writer, re-reads it with the
/// validating reader and only then replaces the document, so an
/// `AccountsDocument` is always valid and [`Self::text`] always means
/// [`Self::config`].
#[derive(Debug, Clone, PartialEq)]
pub struct AccountsDocument {
    file: String,
    text: String,
    config: AccountsConfig,
}

impl AccountsDocument {
    /// An empty document (no file yet).
    pub fn empty(file: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            text: String::new(),
            config: AccountsConfig::default(),
        }
    }

    /// Parses and validates existing file text.
    pub fn parse(text: &str, file: impl Into<String>) -> Result<Self, ConfigError> {
        let file = file.into();
        let config = parse_accounts(text, &file)?;
        Ok(Self {
            file,
            text: text.to_owned(),
            config,
        })
    }

    /// Loads `<dir>/accounts.kdl`; a missing file gives an empty document.
    pub fn load(dir: &Path) -> Result<Self, ConfigError> {
        match crate::backup::read_optional(&dir.join(ACCOUNTS_FILE)) {
            Ok(Some(text)) => Self::parse(&text, ACCOUNTS_FILE),
            Ok(None) => Ok(Self::empty(ACCOUNTS_FILE)),
            Err(e) => Err(ConfigError::io(ACCOUNTS_FILE, &e)),
        }
    }

    /// Writes the document to `<dir>/accounts.kdl` atomically (a temporary
    /// file in the same directory, then a rename), creating `dir` if needed.
    pub fn save(&self, dir: &Path) -> Result<(), ConfigError> {
        crate::backup::write_atomic(&dir.join(ACCOUNTS_FILE), &self.text)
            .map_err(|e| ConfigError::io(&self.file, &e))
    }

    /// The file text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The parsed configuration.
    pub fn config(&self) -> &AccountsConfig {
        &self.config
    }

    /// Replaces the whole configuration, keeping the formatting of
    /// everything that did not change. On error the document is unchanged;
    /// error positions refer to the text the change would have produced.
    pub fn set_config(&mut self, config: AccountsConfig) -> Result<(), ConfigError> {
        if config == self.config {
            return Ok(());
        }
        let text = write::update_text(&self.text, &self.config, &config);
        let parsed = parse_accounts(&text, &self.file)?;
        if parsed != config {
            // The format-preserving path could not express the change
            // exactly (should not happen); fall back to the canonical form
            // of the whole file rather than writing something else.
            tracing::warn!("accounts.kdl: preserving edit mismatched; writing the canonical form");
            let canonical = write::render(&config, &write::detect_indent(&self.text));
            let reparsed = parse_accounts(&canonical, &self.file)?;
            if reparsed != config {
                return Err(ConfigError::at(
                    ConfigErrorKind::Invalid,
                    &nail_api::SourceLocation {
                        file: self.file.clone(),
                        line: 1,
                        column: 1,
                    },
                    crate::l10n::msg("config-accounts-not-writable"),
                ));
            }
            // `reparsed` equals `config`.
            self.text = canonical;
            self.config = config;
            return Ok(());
        }
        // `parsed` equals `config`.
        self.text = text;
        self.config = config;
        Ok(())
    }

    /// Adds `account` (appended) or replaces the account with its id.
    pub fn upsert_account(&mut self, account: AccountConfig) -> Result<(), ConfigError> {
        let mut config = self.config.clone();
        match config.accounts.iter_mut().find(|a| a.id == account.id) {
            Some(existing) => *existing = account,
            None => config.accounts.push(account),
        }
        self.set_config(config)
    }

    /// Removes an account and every reference to it: it leaves the virtual
    /// accounts it was a member of (a virtual account without members is
    /// removed), their folder mappings and identity settings, and the
    /// CalDAV / CardDAV sources that use its login are removed too.
    /// Returns whether the account existed.
    pub fn remove_account(&mut self, id: &AccountId) -> Result<bool, ConfigError> {
        let Some(account) = self.config.account(id).cloned() else {
            return Ok(false);
        };
        let gone: Vec<IdentityId> = account.identities.iter().map(|i| i.id.clone()).collect();
        let mut config = self.config.clone();
        config.accounts.retain(|a| &a.id != id);
        config.pim_sources.retain(|s| s.account() != Some(id));
        config.virtual_accounts.retain_mut(|v| {
            v.members.retain(|m| m != id);
            for mapping in &mut v.folder_mappings {
                mapping.sources.retain(|s| &s.account_id != id);
            }
            v.folder_mappings.retain(|m| !m.sources.is_empty());
            if v.default_identity
                .as_ref()
                .is_some_and(|d| gone.contains(d))
            {
                v.default_identity = None;
            }
            v.identity_rules.retain(|r| !gone.contains(&r.identity));
            !v.members.is_empty()
        });
        self.set_config(config)?;
        Ok(true)
    }

    /// Adds `virtual_account` (appended) or replaces the one with its id.
    pub fn upsert_virtual_account(
        &mut self,
        virtual_account: VirtualAccountConfig,
    ) -> Result<(), ConfigError> {
        let mut config = self.config.clone();
        match config
            .virtual_accounts
            .iter_mut()
            .find(|v| v.id == virtual_account.id)
        {
            Some(existing) => *existing = virtual_account,
            None => config.virtual_accounts.push(virtual_account),
        }
        self.set_config(config)
    }

    /// Removes a virtual account. Returns whether it existed.
    pub fn remove_virtual_account(&mut self, id: &AccountId) -> Result<bool, ConfigError> {
        if self.config.virtual_account(id).is_none() {
            return Ok(false);
        }
        let mut config = self.config.clone();
        config.virtual_accounts.retain(|v| &v.id != id);
        self.set_config(config)?;
        Ok(true)
    }

    /// Adds a PIM source (appended) or replaces the one with its id.
    pub fn upsert_pim_source(&mut self, source: PimSourceConfig) -> Result<(), ConfigError> {
        let mut config = self.config.clone();
        match config.pim_sources.iter_mut().find(|s| s.id == source.id) {
            Some(existing) => *existing = source,
            None => config.pim_sources.push(source),
        }
        self.set_config(config)
    }

    /// Removes a PIM source. Returns whether it existed.
    pub fn remove_pim_source(&mut self, id: &PimSourceId) -> Result<bool, ConfigError> {
        if self.config.pim_source(id).is_none() {
            return Ok(false);
        }
        let mut config = self.config.clone();
        config.pim_sources.retain(|s| &s.id != id);
        self.set_config(config)?;
        Ok(true)
    }

    /// A free PIM source id derived from `hint` (a URL host or a name), by
    /// the account-name rules; `local` is never returned.
    pub fn derive_pim_source_id(&self, hint: &str) -> PimSourceId {
        let taken = |id: &str| {
            id == PimSourceId::LOCAL || self.config.pim_sources.iter().any(|s| s.id.as_str() == id)
        };
        let derived = derive_account_id(hint, taken);
        PimSourceId::new(derived.0)
    }

    /// Whether `id` is used by an account or virtual account.
    pub fn is_taken(&self, id: &str) -> bool {
        self.config.accounts.iter().any(|a| a.id.as_str() == id)
            || self
                .config
                .virtual_accounts
                .iter()
                .any(|v| v.id.as_str() == id)
    }

    /// A free id derived from `hint` (see [`derive_account_id`]).
    pub fn derive_id(&self, hint: &str) -> AccountId {
        derive_account_id(hint, |id| self.is_taken(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_ids() {
        for ok in ["work", "a", "0", "ada-work.example", "x_y", &"a".repeat(64)] {
            assert!(is_valid_account_id(ok), "{ok}");
        }
        for bad in ["", "Work", "-a", ".a", "a b", "a/b", "ä", &"a".repeat(65)] {
            assert!(!is_valid_account_id(bad), "{bad}");
        }
    }

    #[test]
    fn identity_patterns() {
        for ok in ["*@client.com", "*@*.client.com", "boss@client.com"] {
            assert!(is_valid_identity_pattern(ok), "{ok}");
        }
        for bad in [
            "*",
            "*@",
            "*@*",
            "client.com",
            "*@.com",
            "a*@b.com",
            "*@a b",
        ] {
            assert!(!is_valid_identity_pattern(bad), "{bad}");
        }
    }

    #[test]
    fn derived_ids() {
        let none = |_: &str| false;
        assert_eq!(
            derive_account_id("Ada@Work.Example", none).as_str(),
            "ada-work.example"
        );
        assert_eq!(
            derive_account_id("  My Work Mail! ", none).as_str(),
            "my-work-mail"
        );
        assert_eq!(derive_account_id("äöü", none).as_str(), "account");
        assert_eq!(derive_account_id("All", none).as_str(), "all-2");
        let taken = |id: &str| id == "work" || id == "work-2";
        assert_eq!(derive_account_id("work", taken).as_str(), "work-3");
        let long = "x".repeat(100);
        let id = derive_account_id(&long, none);
        assert!(is_valid_account_id(id.as_str()));
        assert!(id.as_str().len() <= MAX_CONFIG_ID_LEN - 4);
    }
}
