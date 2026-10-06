//! PIM sources: where calendars, address books and task lists come from
//! (ADR 0058).
//!
//! * The built-in **local** source ([`crate::ids::PimSourceId::LOCAL`])
//!   holds local calendars, task lists and address books, including the
//!   "Collected" address book of auto-collected recipients. It is not
//!   configured anywhere and cannot be removed.
//! * **ICS subscriptions** (`https:` / `webcal:` feeds) are read-only
//!   calendars refreshed periodically.
//! * **CalDAV** and **CardDAV** servers sync both ways.
//! * **Exchange** (M3c, ADR 0072): one source per Exchange account
//!   (`exchange:<account>`, [`crate::exchange::exchange_pim_source_id`]),
//!   not configured anywhere — it comes and goes with the account; its
//!   calendars, address books and task lists (Microsoft To Do lists
//!   through Graph) sync with the account's engine.
//!
//! Subscriptions and DAV servers are configured in `accounts.kdl`
//! ([`PimSourceConfig`], top-level `calendar-subscription`, `caldav` and
//! `carddav` nodes; ADR 0058), next to the mail accounts whose login a DAV
//! source may share (`account=`). Secrets stay in the OS keychain.
//! Collections (calendars, address books) are discovered or created at
//! runtime and live in the store, not in the file.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::calendar::CalendarComponent;
use crate::accounts::{AuthConfig, SecretKind, SecretRef};
use crate::error::ErrorCode;
use crate::ids::{AccountId, OpId, PimSourceId};
use crate::localized::LocalizedMessage;
use crate::ops::OpState;

/// Default refresh interval of ICS subscriptions, minutes.
pub const DEFAULT_ICS_REFRESH_MINUTES: u32 = 360;
/// Default poll interval of CalDAV / CardDAV sources, minutes.
pub const DEFAULT_DAV_REFRESH_MINUTES: u32 = 15;
/// Shortest refresh / poll interval, minutes.
pub const MIN_REFRESH_MINUTES: u32 = 5;
/// Longest source name (`id`), like account names.
pub const MAX_PIM_SOURCE_ID_LEN: usize = 64;

/// The kind of a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PimSourceKind {
    /// The built-in local source.
    Local,
    /// A read-only ICS / webcal subscription.
    Ics,
    /// A CalDAV server (calendars and task lists).
    Caldav,
    /// A CardDAV server (address books).
    Carddav,
    /// The calendars, contacts and tasks of an Exchange account (M3c).
    Exchange,
}

impl PimSourceKind {
    /// Whether the source holds calendars / task lists.
    pub fn has_calendars(self) -> bool {
        matches!(
            self,
            Self::Local | Self::Ics | Self::Caldav | Self::Exchange
        )
    }

    /// Whether the source holds address books.
    pub fn has_address_books(self) -> bool {
        matches!(self, Self::Local | Self::Carddav | Self::Exchange)
    }

    /// Node name in `accounts.kdl`, `None` for the built-in local source
    /// and Exchange accounts' sources (they follow the account).
    pub fn node_name(self) -> Option<&'static str> {
        match self {
            Self::Local | Self::Exchange => None,
            Self::Ics => Some("calendar-subscription"),
            Self::Caldav => Some("caldav"),
            Self::Carddav => Some("carddav"),
        }
    }
}

/// Kind-specific settings of a configured source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PimSourceSettings {
    /// `calendar-subscription "<id>" url="…"`.
    Ics {
        /// The feed: `https:`, `http:` (loopback hosts only), `webcal:`
        /// or `webcals:` (fetched over HTTPS).
        url: String,
        /// The calendar's colour (see [`super::is_color`]).
        color: Option<String>,
        /// Fire the feed's own alarms (off by default: subscriptions
        /// rarely mean them for you).
        alarms: bool,
        /// Refresh interval in minutes, `null` = the feed's
        /// `REFRESH-INTERVAL` or [`DEFAULT_ICS_REFRESH_MINUTES`].
        refresh_minutes: Option<u32>,
        /// Login for protected feeds, `null` = none.
        auth: Option<AuthConfig>,
    },
    /// `caldav "<id>" …`.
    Caldav {
        /// The mail account whose login (and email domain, for discovery)
        /// the source uses; `null` = standalone (then `auth` is needed).
        account: Option<AccountId>,
        /// The server's URL (`https:`, or `http:` for loopback hosts; context
        /// path or principal); `null` = discover
        /// from the account's email domain (RFC 6764).
        url: Option<String>,
        /// Login, `null` = the account's incoming-server login.
        auth: Option<AuthConfig>,
        /// Poll interval in minutes, `null` = [`DEFAULT_DAV_REFRESH_MINUTES`].
        refresh_minutes: Option<u32>,
    },
    /// `carddav "<id>" …` (same settings as CalDAV).
    Carddav {
        /// See [`PimSourceSettings::Caldav`].
        account: Option<AccountId>,
        /// See [`PimSourceSettings::Caldav`].
        url: Option<String>,
        /// See [`PimSourceSettings::Caldav`].
        auth: Option<AuthConfig>,
        /// See [`PimSourceSettings::Caldav`].
        refresh_minutes: Option<u32>,
    },
}

/// A configured source as written in `accounts.kdl`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PimSourceConfig {
    /// The node's name (`caldav "work-dav"`): 1–64 characters from
    /// `a-z 0-9 - _ .`, unique among sources; `local` is reserved. An empty
    /// id in [`crate::Request::AddPimSource`] lets the core derive one.
    pub id: PimSourceId,
    /// Display name, `null` = derived (the subscription's calendar name,
    /// the server's host).
    #[serde(default)]
    pub name: Option<String>,
    /// `#false` keeps the source but stops syncing it (its data stays).
    #[serde(default = "enabled")]
    pub enabled: bool,
    /// What and where.
    pub settings: PimSourceSettings,
}

fn enabled() -> bool {
    true
}

impl PimSourceConfig {
    /// The source's kind.
    pub fn kind(&self) -> PimSourceKind {
        match self.settings {
            PimSourceSettings::Ics { .. } => PimSourceKind::Ics,
            PimSourceSettings::Caldav { .. } => PimSourceKind::Caldav,
            PimSourceSettings::Carddav { .. } => PimSourceKind::Carddav,
        }
    }

    /// The mail account a DAV source belongs to.
    pub fn account(&self) -> Option<&AccountId> {
        match &self.settings {
            PimSourceSettings::Ics { .. } => None,
            PimSourceSettings::Caldav { account, .. }
            | PimSourceSettings::Carddav { account, .. } => account.as_ref(),
        }
    }

    /// The configured URL.
    pub fn url(&self) -> Option<&str> {
        match &self.settings {
            PimSourceSettings::Ics { url, .. } => Some(url),
            PimSourceSettings::Caldav { url, .. } | PimSourceSettings::Carddav { url, .. } => {
                url.as_deref()
            }
        }
    }

    /// The configured login (DAV sources without one use their account's).
    pub fn auth(&self) -> Option<&AuthConfig> {
        match &self.settings {
            PimSourceSettings::Ics { auth, .. }
            | PimSourceSettings::Caldav { auth, .. }
            | PimSourceSettings::Carddav { auth, .. } => auth.as_ref(),
        }
    }

    /// The refresh / poll interval in minutes, defaults applied.
    pub fn refresh_minutes(&self) -> u32 {
        match &self.settings {
            PimSourceSettings::Ics {
                refresh_minutes, ..
            } => refresh_minutes.unwrap_or(DEFAULT_ICS_REFRESH_MINUTES),
            PimSourceSettings::Caldav {
                refresh_minutes, ..
            }
            | PimSourceSettings::Carddav {
                refresh_minutes, ..
            } => refresh_minutes.unwrap_or(DEFAULT_DAV_REFRESH_MINUTES),
        }
    }

    /// The default keychain reference of this source's secret:
    /// `keychain:nail/pim/<id>/<kind slot>` (separate from account
    /// secrets, so a source and an account may share a name).
    pub fn default_secret(&self, kind: SecretKind) -> SecretRef {
        SecretRef(format!(
            "{}{}/pim/{}/{}",
            SecretRef::SCHEME,
            SecretRef::SERVICE,
            self.id,
            kind.slot()
        ))
    }

    /// Problems with the configuration (empty = valid). Whether `account`
    /// exists is checked by the reader of `accounts.kdl`.
    pub fn issues(&self) -> Vec<PimSourceIssue> {
        let mut out = Vec::new();
        if !is_valid_pim_source_id(self.id.as_str()) {
            out.push(PimSourceIssue::InvalidId);
        } else if self.id.as_str() == PimSourceId::LOCAL {
            out.push(PimSourceIssue::ReservedId);
        }
        if self.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
            out.push(PimSourceIssue::EmptyName);
        }
        match &self.settings {
            PimSourceSettings::Ics {
                url,
                color,
                refresh_minutes,
                ..
            } => {
                if !(super::is_https_or_loopback_url(url)
                    || super::has_url_scheme(url, &["webcal", "webcals"]))
                {
                    out.push(PimSourceIssue::InvalidUrl);
                }
                if color.as_deref().is_some_and(|c| !super::is_color(c)) {
                    out.push(PimSourceIssue::InvalidColor);
                }
                if refresh_minutes.is_some_and(|m| m < MIN_REFRESH_MINUTES) {
                    out.push(PimSourceIssue::RefreshTooShort);
                }
            }
            PimSourceSettings::Caldav {
                account,
                url,
                auth,
                refresh_minutes,
            }
            | PimSourceSettings::Carddav {
                account,
                url,
                auth,
                refresh_minutes,
            } => {
                if url
                    .as_deref()
                    .is_some_and(|u| !super::is_https_or_loopback_url(u))
                {
                    out.push(PimSourceIssue::InvalidUrl);
                }
                if account.is_none() && url.is_none() {
                    out.push(PimSourceIssue::MissingUrl);
                }
                if account.is_none() && auth.is_none() {
                    out.push(PimSourceIssue::MissingAuth);
                }
                if refresh_minutes.is_some_and(|m| m < MIN_REFRESH_MINUTES) {
                    out.push(PimSourceIssue::RefreshTooShort);
                }
            }
        }
        out
    }
}

/// Whether `id` is a valid source name: the account-name rules (1–64
/// characters from `a-z 0-9 - _ .`, starting with a letter or digit).
pub fn is_valid_pim_source_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_PIM_SOURCE_ID_LEN
        && id
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
}

/// The `https:` URL an ICS subscription is fetched from: `webcal:` and
/// `webcals:` become `https:`; other URLs are returned unchanged.
pub fn ics_fetch_url(url: &str) -> String {
    for scheme in ["webcals://", "webcal://"] {
        if url.len() >= scheme.len() && url[..scheme.len()].eq_ignore_ascii_case(scheme) {
            return format!("https://{}", &url[scheme.len()..]);
        }
    }
    url.to_owned()
}

/// A problem with a [`PimSourceConfig`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PimSourceIssue {
    /// The id breaks the naming rules.
    InvalidId,
    /// The id is `local`.
    ReservedId,
    /// The name is blank.
    EmptyName,
    /// The URL is not `https:` (or `http:` on a loopback host; `webcal:` /
    /// `webcals:` for subscriptions).
    InvalidUrl,
    /// A standalone DAV source (no `account`) without a URL.
    MissingUrl,
    /// A standalone DAV source (no `account`) without a login.
    MissingAuth,
    /// The colour is invalid.
    InvalidColor,
    /// The refresh interval is below [`MIN_REFRESH_MINUTES`].
    RefreshTooShort,
}

impl PimSourceIssue {
    /// All issues.
    pub const ALL: [PimSourceIssue; 8] = [
        Self::InvalidId,
        Self::ReservedId,
        Self::EmptyName,
        Self::InvalidUrl,
        Self::MissingUrl,
        Self::MissingAuth,
        Self::InvalidColor,
        Self::RefreshTooShort,
    ];

    /// Fluent message id, e.g. `pim-source-issue-invalid-url`.
    pub fn message_id(self) -> &'static str {
        match self {
            Self::InvalidId => "pim-source-issue-invalid-id",
            Self::ReservedId => "pim-source-issue-reserved-id",
            Self::EmptyName => "pim-source-issue-empty-name",
            Self::InvalidUrl => "pim-source-issue-invalid-url",
            Self::MissingUrl => "pim-source-issue-missing-url",
            Self::MissingAuth => "pim-source-issue-missing-auth",
            Self::InvalidColor => "pim-source-issue-invalid-color",
            Self::RefreshTooShort => "pim-source-issue-refresh-too-short",
        }
    }
}

/// Sync state of a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PimSourceState {
    /// Up to date (or local).
    Idle,
    /// Syncing now.
    Syncing,
    /// The server cannot be reached; local changes wait in the queue.
    Offline,
    /// The server refused the login.
    AuthError,
    /// The last sync failed for another reason.
    Error,
    /// Switched off (`enabled #false`).
    Disabled,
}

impl PimSourceState {
    /// All states.
    pub const ALL: [PimSourceState; 6] = [
        Self::Idle,
        Self::Syncing,
        Self::Offline,
        Self::AuthError,
        Self::Error,
        Self::Disabled,
    ];

    /// Fluent message id of the state's label, e.g. `pim-source-state-idle`.
    pub fn message_id(self) -> &'static str {
        match self {
            Self::Idle => "pim-source-state-idle",
            Self::Syncing => "pim-source-state-syncing",
            Self::Offline => "pim-source-state-offline",
            Self::AuthError => "pim-source-state-auth-error",
            Self::Error => "pim-source-state-error",
            Self::Disabled => "pim-source-state-disabled",
        }
    }
}

/// A source with its status ([`crate::Request::ListPimSources`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PimSource {
    /// The source.
    pub id: PimSourceId,
    /// Its kind.
    pub kind: PimSourceKind,
    /// Display name (resolved).
    pub name: String,
    /// The mail account it belongs to.
    pub account_id: Option<AccountId>,
    /// Whether it syncs.
    pub enabled: bool,
    /// Its configuration (`null` for the local source).
    pub config: Option<PimSourceConfig>,
    /// Sync state.
    pub state: PimSourceState,
    /// Last successful sync (RFC 3339 UTC), `null` = never.
    pub last_sync_at: Option<String>,
    /// Error class of the last failure, if `state` is an error.
    pub error: Option<ErrorCode>,
    /// Diagnostic detail (English).
    pub message: Option<String>,
    /// The user-facing text (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
}

/// Sync state of one item (event, task, contact, group).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PimItemSync {
    /// Same as on the server (or local).
    #[default]
    Synced,
    /// A local change waits to be written to the server.
    Pending,
    /// The server changed it too: the server's version is shown and the
    /// local version is kept as a conflict copy until resolved
    /// ([`crate::Request::ResolvePimConflict`]).
    Conflict,
}

/// Progress of one queued PIM operation, pushed as
/// [`crate::Event::PimOpStatus`] (the PIM counterpart of
/// [`crate::ops::OpStatus`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PimOpStatus {
    /// The operation (as returned by the write request).
    pub op_id: OpId,
    /// The source it is replayed to.
    pub source_id: PimSourceId,
    /// New state.
    pub state: OpState,
    /// Error class when failed (or a retry is pending).
    pub error: Option<ErrorCode>,
    /// Diagnostic detail (English).
    pub message: Option<String>,
    /// The user-facing text (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
}

/// A collection found on a server ([`crate::Request::TestPimSource`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DiscoveredCollection {
    /// Its URL.
    pub url: String,
    /// Its display name.
    pub name: String,
    /// What it holds: calendar components for calendars, empty for
    /// address books.
    pub components: Vec<CalendarComponent>,
    /// Whether it is an address book.
    pub address_book: bool,
    /// Whether the login may only read it.
    pub read_only: bool,
    /// Its colour, if the server has one.
    pub color: Option<String>,
}

/// The result of [`crate::Request::TestPimSource`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PimSourceTestResult {
    /// Whether the source can be used as configured.
    pub ok: bool,
    /// The URL found by discovery (to save in the configuration), if any.
    pub discovered_url: Option<String>,
    /// Collections found (subscriptions: one calendar with the feed's name).
    pub collections: Vec<DiscoveredCollection>,
    /// Error class when not `ok`.
    pub error: Option<ErrorCode>,
    /// Diagnostic detail (English).
    pub message: Option<String>,
    /// The user-facing text (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dav(account: Option<&str>, url: Option<&str>) -> PimSourceConfig {
        PimSourceConfig {
            id: "work-dav".into(),
            name: None,
            enabled: true,
            settings: PimSourceSettings::Caldav {
                account: account.map(Into::into),
                url: url.map(Into::into),
                auth: None,
                refresh_minutes: None,
            },
        }
    }

    #[test]
    fn source_configs_validate() {
        assert_eq!(dav(Some("work"), None).issues().len(), 0);
        assert_eq!(
            dav(None, None).issues(),
            [PimSourceIssue::MissingUrl, PimSourceIssue::MissingAuth]
        );
        assert_eq!(
            dav(Some("work"), Some("ftp://dav.example.org")).issues(),
            [PimSourceIssue::InvalidUrl]
        );
        assert_eq!(
            dav(Some("work"), Some("http://dav.example.org")).issues(),
            [PimSourceIssue::InvalidUrl]
        );
        assert!(
            dav(Some("work"), Some("http://127.0.0.1:5232/"))
                .issues()
                .is_empty(),
            "{:?}",
            dav(Some("work"), Some("http://127.0.0.1:5232/")).issues()
        );
        let mut local = dav(Some("work"), None);
        local.id = "local".into();
        assert_eq!(local.issues(), [PimSourceIssue::ReservedId]);
        local.id = "Work".into();
        assert_eq!(local.issues(), [PimSourceIssue::InvalidId]);

        let ics = PimSourceConfig {
            id: "holidays".into(),
            name: Some("Holidays".into()),
            enabled: true,
            settings: PimSourceSettings::Ics {
                url: "webcal://example.org/holidays.ics".into(),
                color: Some("#ff0000".into()),
                alarms: false,
                refresh_minutes: Some(1),
                auth: None,
            },
        };
        assert_eq!(ics.issues(), [PimSourceIssue::RefreshTooShort]);
        assert_eq!(ics.kind(), PimSourceKind::Ics);
        assert_eq!(ics.url(), Some("webcal://example.org/holidays.ics"));
        assert_eq!(ics.account(), None);
        assert_eq!(ics.refresh_minutes(), 1);
        assert_eq!(
            dav(None, None).refresh_minutes(),
            DEFAULT_DAV_REFRESH_MINUTES
        );
        assert_eq!(
            ics.default_secret(SecretKind::Password).as_str(),
            "keychain:nail/pim/holidays/password"
        );
        assert!(SecretRef::parse(ics.default_secret(SecretKind::OAuth2).as_str()).is_ok());
    }

    #[test]
    fn source_wire_shape() {
        let config: PimSourceConfig = serde_json::from_value(json!({
            "id": "work-dav",
            "settings": {"kind": "caldav", "account": "work", "url": null, "auth": null, "refresh_minutes": null}
        }))
        .unwrap();
        assert_eq!(config, dav(Some("work"), None));
        assert!(config.enabled);
        assert_eq!(
            serde_json::to_value(PimSourceKind::Carddav).unwrap(),
            json!("carddav")
        );
        assert_eq!(
            PimSourceKind::Ics.node_name(),
            Some("calendar-subscription")
        );
        assert!(PimSourceKind::Local.has_calendars() && PimSourceKind::Local.has_address_books());
        assert!(!PimSourceKind::Ics.has_address_books());
        assert!(!PimSourceKind::Carddav.has_calendars());
    }

    #[test]
    fn webcal_urls_become_https() {
        assert_eq!(
            ics_fetch_url("webcal://a.example/x.ics"),
            "https://a.example/x.ics"
        );
        assert_eq!(
            ics_fetch_url("WEBCALS://a.example/x.ics"),
            "https://a.example/x.ics"
        );
        assert_eq!(
            ics_fetch_url("http://127.0.0.1:8080/x.ics"),
            "http://127.0.0.1:8080/x.ics"
        );
    }

    #[test]
    fn issue_and_state_ids() {
        for issue in PimSourceIssue::ALL {
            let wire = serde_json::to_value(issue).unwrap();
            assert_eq!(
                format!("pim-source-issue-{}", wire.as_str().unwrap()),
                issue.message_id()
            );
        }
        for state in PimSourceState::ALL {
            let wire = serde_json::to_value(state).unwrap();
            assert_eq!(
                format!("pim-source-state-{}", wire.as_str().unwrap()),
                state.message_id()
            );
        }
        assert!(is_valid_pim_source_id("a.b-c_d"));
        assert!(!is_valid_pim_source_id("-a"));
    }
}
