//! Server-side rules and vacation replies through Sieve / ManageSieve
//! (design.md, "Filters & rules" → "Server-side execution"; M2c, ADR 0043).
//!
//! * **ManageSieve settings** are part of an IMAP account in
//!   `accounts.kdl` ([`ManageSieveConfig`], the `sieve` node). The login is
//!   always the account's IMAP login (same user name, password or OAuth2
//!   token); only host, port, TLS and the script name can differ.
//! * **Placement per rule** ([`RuleSieveStatus`]): a rule whose triggers,
//!   conditions and actions all have a Sieve equivalent runs on the
//!   **server** once its account's script is uploaded; a rule that could
//!   but does not (ManageSieve off or unavailable, other triggers, a scope
//!   the server cannot see) runs on the **client**; a rule with something
//!   Sieve cannot express is **not expressible** and always runs on the
//!   client — [`RuleSieveStatus::reasons`] says why. A rule never runs in
//!   both places for the same message.
//! * **Vacation** ([`VacationSettings`]) is compiled into the same script
//!   with the Sieve `vacation` extension (RFC 5230).
//! * Nail owns one script per account ([`ManageSieveConfig::script_name`],
//!   default [`DEFAULT_SCRIPT_NAME`]) and makes it the active one. Other
//!   scripts on the server are left alone (listed in
//!   [`SieveAccountStatus::other_scripts`]).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::accounts::{ServerEndpoint, TlsMode};
use crate::error::ErrorCode;
use crate::ids::{AccountId, RuleId};
use crate::localized::LocalizedMessage;

/// The ManageSieve port (RFC 5804).
pub const MANAGESIEVE_DEFAULT_PORT: u16 = 4190;

/// The name of the script Nail uploads when none is configured.
pub const DEFAULT_SCRIPT_NAME: &str = "nail";

/// Longest script name Nail writes (RFC 5804 allows more; servers differ).
pub const MAX_SCRIPT_NAME_LEN: usize = 128;

/// ManageSieve settings of an IMAP account: the `sieve` node inside
/// `account` in `accounts.kdl`.
///
/// ```kdl
/// account "work" {
///     imap host="imap.example.com" …
///     sieve host="sieve.example.com" port=4190 tls="starttls" script="nail"
/// }
/// ```
///
/// Every field is optional; `null` means the documented default.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct ManageSieveConfig {
    /// Upload rules and vacation replies. Default true (the node exists);
    /// `false` keeps the settings but runs every rule on the client.
    pub enabled: Option<bool>,
    /// Server host; `null` = the account's IMAP host.
    pub host: Option<String>,
    /// Port; `null` = 4190.
    pub port: Option<u16>,
    /// Transport security; `null` = `starttls` (RFC 5804 has no implicit
    /// TLS port; `implicit` is accepted for servers behind a TLS proxy).
    pub tls: Option<TlsMode>,
    /// Name of Nail's script on the server; `null` = `"nail"`.
    pub script_name: Option<String>,
}

impl ManageSieveConfig {
    /// Whether uploading is on.
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    /// The effective host, port and TLS mode for an account whose IMAP
    /// server is `imap`. The login ([`ServerEndpoint::auth`]) is the IMAP
    /// login.
    pub fn endpoint(&self, imap: &ServerEndpoint) -> ServerEndpoint {
        ServerEndpoint {
            host: self.host.clone().unwrap_or_else(|| imap.host.clone()),
            port: self.port.unwrap_or(MANAGESIEVE_DEFAULT_PORT),
            tls: self.tls.unwrap_or(TlsMode::Starttls),
            auth: imap.auth.clone(),
        }
    }

    /// The effective script name.
    pub fn effective_script_name(&self) -> &str {
        self.script_name.as_deref().unwrap_or(DEFAULT_SCRIPT_NAME)
    }

    /// Problems with the settings (empty = valid).
    pub fn issues(&self) -> Vec<ManageSieveIssue> {
        let mut out = Vec::new();
        if let Some(host) = &self.host
            && (host.trim().is_empty() || host.contains(char::is_whitespace))
        {
            out.push(ManageSieveIssue::Host);
        }
        if self.port == Some(0) {
            out.push(ManageSieveIssue::Port);
        }
        if let Some(name) = &self.script_name
            && !is_script_name(name)
        {
            out.push(ManageSieveIssue::ScriptName);
        }
        out
    }
}

/// Whether `name` is a script name Nail accepts: 1–128 characters, no
/// control characters, no `/` or `\`, not only whitespace.
pub fn is_script_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.chars().count() <= MAX_SCRIPT_NAME_LEN
        && !name
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
}

/// A problem with [`ManageSieveConfig`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ManageSieveIssue {
    /// The host is empty or contains spaces.
    Host,
    /// Port 0.
    Port,
    /// The script name is empty, too long or has control characters or
    /// slashes.
    ScriptName,
}

impl ManageSieveIssue {
    /// All issues.
    pub const ALL: [ManageSieveIssue; 3] = [Self::Host, Self::Port, Self::ScriptName];

    /// Fluent message id, e.g. `sieve-issue-script-name`.
    pub fn message_id(self) -> &'static str {
        match self {
            Self::Host => "sieve-issue-host",
            Self::Port => "sieve-issue-port",
            Self::ScriptName => "sieve-issue-script-name",
        }
    }
}

/// Where a rule runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum RulePlacement {
    /// Compiled into the account's Sieve script and uploaded: the server
    /// runs it, also while Nail is offline. The client does not run it on
    /// arrival (manual runs still work).
    Server,
    /// Expressible in Sieve but run by Nail (ManageSieve off, unavailable
    /// or the script not uploaded yet, or a trigger / scope the server
    /// cannot handle).
    Client,
    /// Uses something Sieve cannot express; always run by Nail.
    NotExpressible,
}

/// Where one rule runs for one account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RuleSieveStatus {
    /// The rule.
    pub rule_id: RuleId,
    /// Where it runs.
    pub placement: RulePlacement,
    /// Why it is `client` or `not-expressible` (e.g. `sieve-reason-query`
    /// with the condition's position); empty for `server`.
    #[serde(default)]
    pub reasons: Vec<LocalizedMessage>,
}

/// State of an account's Sieve script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum SieveState {
    /// No `sieve` node, uploading switched off, or not an IMAP account.
    Disabled,
    /// The server has no ManageSieve service (connection refused, no
    /// `SIEVE` capability) — every rule runs on the client.
    Unavailable,
    /// Rules or vacation changed and the new script is not uploaded yet
    /// (offline, or waiting for the upload).
    Pending,
    /// Uploading now.
    Uploading,
    /// The server runs the current script.
    InSync,
    /// The last upload failed (see `error`); retried on the next change or
    /// [`crate::Request::SyncSieve`].
    Failed,
}

/// The Sieve state of one account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SieveAccountStatus {
    /// The account.
    pub account_id: AccountId,
    /// Script state.
    pub state: SieveState,
    /// Nail's script name on the server.
    pub script_name: String,
    /// When the current script was uploaded (RFC 3339), `null` = never.
    pub uploaded_at: Option<String>,
    /// The server's Sieve extensions (`SIEVE` capability, lowercase), as
    /// last seen; empty when never connected.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Other scripts on the server (left alone), as last seen.
    #[serde(default)]
    pub other_scripts: Vec<String>,
    /// Where each rule that applies to the account runs, in `rules.kdl`
    /// order.
    #[serde(default)]
    pub rules: Vec<RuleSieveStatus>,
    /// Whether the uploaded script contains an active vacation reply.
    #[serde(default)]
    pub vacation_active: bool,
    /// Error class of the last failure (`state` = `failed` or
    /// `unavailable`), else `null`.
    pub error: Option<ErrorCode>,
    /// Diagnostic detail (English), else `null`.
    pub message: Option<String>,
    /// The user-facing text (Fluent id + arguments) of the failure, e.g.
    /// the server's `CHECKSCRIPT` complaint; else `null` (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
}

/// A vacation (out-of-office) reply, compiled to Sieve `vacation`
/// (RFC 5230) with dates through the `date` extension (RFC 5260) when the
/// server has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct VacationSettings {
    /// Whether replies are sent.
    pub enabled: bool,
    /// Subject of the reply; empty = the server's default (usually
    /// `Auto: <original subject>`).
    pub subject: String,
    /// Plain-text body of the reply.
    pub body: String,
    /// First day (`YYYY-MM-DD`, the account's local time), `null` = now.
    pub start_date: Option<String>,
    /// Last day (inclusive), `null` = until switched off.
    pub end_date: Option<String>,
    /// Reply to the same sender at most once in this many days (1–365).
    pub days: u32,
    /// Further own addresses mail may be addressed to (`:addresses`); the
    /// account's identities are added automatically.
    pub addresses: Vec<String>,
    /// Sender address of the reply (`:from`), `null` = the server's
    /// choice (the address the mail was sent to).
    pub from: Option<String>,
    /// Do not reply to mailing lists and bulk mail (`List-Id`,
    /// `Precedence: bulk / list / junk`, `Auto-Submitted`).
    pub skip_lists: bool,
}

impl Default for VacationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            subject: String::new(),
            body: String::new(),
            start_date: None,
            end_date: None,
            days: Self::DEFAULT_DAYS,
            addresses: Vec::new(),
            from: None,
            skip_lists: true,
        }
    }
}

impl VacationSettings {
    /// Default of [`Self::days`].
    pub const DEFAULT_DAYS: u32 = 7;
    /// Largest [`Self::days`].
    pub const MAX_DAYS: u32 = 365;

    /// Problems with the settings (empty = valid). Disabled settings are
    /// still checked, except for the empty body.
    pub fn issues(&self) -> Vec<VacationIssue> {
        let mut out = Vec::new();
        if self.enabled && self.body.trim().is_empty() {
            out.push(VacationIssue::EmptyBody);
        }
        if self.days == 0 || self.days > Self::MAX_DAYS {
            out.push(VacationIssue::Days);
        }
        let start = self.start_date.as_deref();
        let end = self.end_date.as_deref();
        if start.is_some_and(|d| !is_iso_date(d)) || end.is_some_and(|d| !is_iso_date(d)) {
            out.push(VacationIssue::Date);
        } else if let (Some(s), Some(e)) = (start, end)
            && e < s
        {
            out.push(VacationIssue::EndBeforeStart);
        }
        let bad_address = |a: &str| {
            let a = a.trim();
            a.is_empty() || !a.contains('@') || a.contains(char::is_whitespace)
        };
        if self.addresses.iter().any(|a| bad_address(a))
            || self.from.as_deref().is_some_and(bad_address)
        {
            out.push(VacationIssue::Address);
        }
        if self.subject.contains(['\r', '\n']) {
            out.push(VacationIssue::Subject);
        }
        out
    }
}

/// Whether `text` is a calendar date `YYYY-MM-DD` (month 1–12, day 1–31;
/// the day is not checked against the month).
pub fn is_iso_date(text: &str) -> bool {
    let b = text.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    if !(digits(0..4) && digits(5..7) && digits(8..10)) {
        return false;
    }
    let month: u32 = text[5..7].parse().unwrap_or(0);
    let day: u32 = text[8..10].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day)
}

/// A problem with [`VacationSettings`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum VacationIssue {
    /// Enabled without a body.
    EmptyBody,
    /// `days` outside 1–365.
    Days,
    /// A date that is not `YYYY-MM-DD`.
    Date,
    /// The end date is before the start date.
    EndBeforeStart,
    /// An address in `addresses` or `from` is not an address.
    Address,
    /// The subject contains a line break.
    Subject,
}

impl VacationIssue {
    /// Fluent message id, e.g. `vacation-issue-empty-body`.
    pub fn message_id(self) -> &'static str {
        match self {
            Self::EmptyBody => "vacation-issue-empty-body",
            Self::Days => "vacation-issue-days",
            Self::Date => "vacation-issue-date",
            Self::EndBeforeStart => "vacation-issue-end-before-start",
            Self::Address => "vacation-issue-address",
            Self::Subject => "vacation-issue-subject",
        }
    }

    /// All issues.
    pub const ALL: [VacationIssue; 6] = [
        Self::EmptyBody,
        Self::Days,
        Self::Date,
        Self::EndBeforeStart,
        Self::Address,
        Self::Subject,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{AuthConfig, SecretRef};
    use serde_json::json;

    fn imap() -> ServerEndpoint {
        ServerEndpoint {
            host: "imap.example.com".into(),
            port: 993,
            tls: TlsMode::Implicit,
            auth: AuthConfig::Password {
                username: "ada".into(),
                secret: Some(SecretRef("keychain:nail/work".into())),
                mechanism: None,
            },
        }
    }

    #[test]
    fn managesieve_defaults_follow_the_imap_login() {
        let config: ManageSieveConfig = serde_json::from_value(json!({})).unwrap();
        assert!(config.is_enabled() && config.issues().is_empty());
        let endpoint = config.endpoint(&imap());
        assert_eq!(endpoint.host, "imap.example.com");
        assert_eq!(endpoint.port, MANAGESIEVE_DEFAULT_PORT);
        assert_eq!(endpoint.tls, TlsMode::Starttls);
        assert_eq!(endpoint.auth, imap().auth);
        assert_eq!(config.effective_script_name(), "nail");

        let custom = ManageSieveConfig {
            enabled: Some(false),
            host: Some("sieve.example.com".into()),
            port: Some(2000),
            tls: Some(TlsMode::Implicit),
            script_name: Some("mine".into()),
        };
        assert!(!custom.is_enabled());
        assert_eq!(custom.endpoint(&imap()).host, "sieve.example.com");
        assert_eq!(custom.effective_script_name(), "mine");
    }

    #[test]
    fn managesieve_issues() {
        let bad = ManageSieveConfig {
            host: Some("a b".into()),
            port: Some(0),
            script_name: Some("a/b".into()),
            ..ManageSieveConfig::default()
        };
        assert_eq!(
            bad.issues(),
            [
                ManageSieveIssue::Host,
                ManageSieveIssue::Port,
                ManageSieveIssue::ScriptName
            ]
        );
        assert!(!is_script_name(" ") && !is_script_name("x\u{7}"));
        assert!(!is_script_name(&"x".repeat(129)) && is_script_name("Nail rules"));
        assert_eq!(
            ManageSieveIssue::ScriptName.message_id(),
            "sieve-issue-script-name"
        );
    }

    #[test]
    fn vacation_defaults_and_issues() {
        let v: VacationSettings = serde_json::from_value(json!({})).unwrap();
        assert_eq!(v, VacationSettings::default());
        assert!(!v.enabled && v.skip_lists && v.days == 7 && v.issues().is_empty());

        let on = VacationSettings {
            enabled: true,
            ..VacationSettings::default()
        };
        assert_eq!(on.issues(), [VacationIssue::EmptyBody]);

        let bad = VacationSettings {
            days: 0,
            start_date: Some("2026-10-10".into()),
            end_date: Some("2026-10-01".into()),
            addresses: vec!["nope".into()],
            subject: "a\nb".into(),
            ..VacationSettings::default()
        };
        assert_eq!(
            bad.issues(),
            [
                VacationIssue::Days,
                VacationIssue::EndBeforeStart,
                VacationIssue::Address,
                VacationIssue::Subject
            ]
        );
        let bad_date = VacationSettings {
            start_date: Some("2026-13-01".into()),
            ..VacationSettings::default()
        };
        assert_eq!(bad_date.issues(), [VacationIssue::Date]);
        assert!(is_iso_date("2026-02-28") && !is_iso_date("2026-2-28"));
        let ids: std::collections::HashSet<_> =
            VacationIssue::ALL.iter().map(|i| i.message_id()).collect();
        assert_eq!(ids.len(), VacationIssue::ALL.len());
    }

    #[test]
    fn status_serializes() {
        let status = SieveAccountStatus {
            account_id: "work".into(),
            state: SieveState::InSync,
            script_name: "nail".into(),
            uploaded_at: Some("2026-09-30T08:00:00Z".into()),
            extensions: vec!["fileinto".into(), "vacation".into()],
            other_scripts: vec![],
            rules: vec![RuleSieveStatus {
                rule_id: "big".into(),
                placement: RulePlacement::NotExpressible,
                reasons: vec![LocalizedMessage::new("sieve-reason-age")],
            }],
            vacation_active: false,
            error: None,
            message: None,
            localized: None,
        };
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(value["state"], json!("in-sync"));
        assert_eq!(value["rules"][0]["placement"], json!("not-expressible"));
        assert!(value.get("localized").is_none());
        assert_eq!(
            serde_json::from_value::<SieveAccountStatus>(value).unwrap(),
            status
        );
    }
}
