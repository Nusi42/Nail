//! The remote UI: settings, users and credentials, paired devices,
//! sessions, pairing codes, the login audit log and the browser's login
//! protocol (design.md, "Remote UI" and "Remote UI security"; plan M2b;
//! ADR 0036).
//!
//! * **Settings and status** ([`RemoteUiSettings`], [`RemoteUiStatus`]):
//!   off by default, loopback by default, TLS required for any
//!   non-loopback address, internet mode (and with it ACME) **locked**
//!   until the penetration-test gate is recorded
//!   ([`internet_mode_lock`]).
//! * **Users** ([`RemoteUser`]): a login name with credentials — an
//!   Argon2id password hash, a TOTP secret (kept in the OS keychain, only
//!   its reference is stored) and passkeys ([`Passkey`], WebAuthn). No
//!   default credentials: a user without any credential cannot log in.
//! * **Devices** ([`RemoteDevice`]): a browser paired with a one-time
//!   [`PairingCode`] created (and thereby approved) on the desktop.
//! * **Sessions** ([`RemoteSession`]): short-lived, device-bound logins in
//!   `HttpOnly; Secure; SameSite=Strict` cookies, rotated while in use;
//!   each carries a [`RemoteGrant`] that never exceeds its device's and
//!   user's grant.
//! * **Audit** ([`RemoteAuditEntry`]): every login, failed login, pairing,
//!   credential change and remote action.
//! * **Login protocol** ([`AuthRequest`] / [`AuthResponse`]): JSON over
//!   `POST` [`AUTH_PATH`], the one HTTP route besides the bundle, the
//!   WebSocket, blobs and the remote-content proxy. It exists because
//!   `HttpOnly` cookies can only be set by HTTP responses.
//!
//! Administration (everything in [`crate::Request`] from this module) is
//! done on the desktop, or from a remote session whose grant has `admin`
//! and that re-authenticated recently (error code `reauth_required`).
//!
//! Secrets never appear in this module's read types: passwords, TOTP
//! secrets, pairing codes and session tokens go in once (a request or the
//! login protocol) and are stored hashed (passwords: Argon2id PHC string;
//! pairing codes and session tokens: SHA-256) or, for TOTP, in the OS
//! keychain. The two exceptions are shown once: [`TotpEnrollment`] (to
//! scan) and the code in [`crate::Response::PairingCodeCreated`].

use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::ErrorCode;
use crate::ids::{PairingId, PasskeyId, RemoteDeviceId, RemoteSessionId, RemoteUserId};
use crate::localized::LocalizedMessage;
use crate::mcp::{McpGrant, McpScope};

// ---------------------------------------------------------------------------
// Wire constants of the remote UI (shared by nail-server, nail-remote-auth
// and the UI)
// ---------------------------------------------------------------------------

/// Default port of the remote UI (the same as `nail serve`).
pub const DEFAULT_REMOTE_UI_PORT: u16 = 4780;

/// The login protocol's route: `POST` an [`AuthRequest`], get an
/// [`AuthResponse`].
pub const AUTH_PATH: &str = "/api/auth";

/// The answer to `POST` [`AUTH_PATH`] from a server without a login (the
/// desktop's loopback `nail serve`): `204 No Content`, so the UI opens the
/// app at once instead of showing a login (`ui/src/features/remote/authClient.ts`
/// `NO_LOGIN_STATUS`).
pub const NO_LOGIN_STATUS: u16 = 204;

/// Session cookie (`HttpOnly; Secure; SameSite=Strict; Path=/`). The
/// `__Host-` prefix pins it to the exact origin.
pub const SESSION_COOKIE: &str = "__Host-nail-session";

/// Device cookie: a long-lived random secret that binds sessions to the
/// paired browser (`HttpOnly; Secure; SameSite=Strict; Path=/`).
pub const DEVICE_COOKIE: &str = "__Host-nail-device";

/// Header carrying the CSRF token from [`AuthStatus::csrf_token`] on every
/// state-changing request (the login protocol and the WebSocket upgrade's
/// first frame are checked by `Origin` as well).
pub const CSRF_HEADER: &str = "x-nail-csrf";

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

/// How the remote UI serves TLS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteTlsMode {
    /// A self-signed certificate made by Nail; browsers pin its
    /// fingerprint at pairing (the LAN default).
    #[default]
    SelfSigned,
    /// A certificate from an ACME CA (Let's Encrypt). Needs internet mode,
    /// which is locked in M2.
    Acme,
    /// Plain HTTP. Only allowed on a loopback address (behind a local
    /// reverse proxy or tunnel that terminates TLS).
    Off,
}

/// ACME settings (used only in internet mode).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AcmeSettings {
    /// Domain names for the certificate; the first is the primary name.
    pub domains: Vec<String>,
    /// Contact address given to the CA, `null` = none.
    pub contact_email: Option<String>,
    /// The CA's directory URL.
    pub directory_url: String,
    /// The user accepted the CA's terms of service.
    pub accept_terms: bool,
}

/// Let's Encrypt's production directory.
pub const LETS_ENCRYPT_DIRECTORY: &str = "https://acme-v02.api.letsencrypt.org/directory";

impl Default for AcmeSettings {
    fn default() -> Self {
        Self {
            domains: Vec::new(),
            contact_email: None,
            directory_url: LETS_ENCRYPT_DIRECTORY.to_owned(),
            accept_terms: false,
        }
    }
}

/// Settings of the remote UI (stored by the core; the desktop's Settings
/// → Remote access edits them). The default is off, on loopback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct RemoteUiSettings {
    /// Serve the remote UI.
    pub enabled: bool,
    /// Address to listen on (an IP address; `0.0.0.0` / `::` = every
    /// interface). Anything but loopback needs TLS.
    pub bind_address: String,
    /// TCP port.
    pub port: u16,
    /// Host name browsers use to reach Nail (put into the self-signed
    /// certificate, and the WebAuthn relying-party id: passkeys need a
    /// name, not an IP address). `null` = the computer's name.
    pub host_name: Option<String>,
    /// TLS mode.
    pub tls: RemoteTlsMode,
    /// ACME settings (only with [`RemoteTlsMode::Acme`]).
    pub acme: AcmeSettings,
    /// Serve beyond the LAN. **Locked** until the penetration-test gate is
    /// recorded ([`internet_mode_lock`]); the core refuses `true`.
    pub internet_mode: bool,
    /// Networks allowed to connect (CIDR, e.g. `192.168.1.0/24`); empty =
    /// any address that reaches the port.
    pub allowed_networks: Vec<String>,
    /// A session ends after this many minutes without a request.
    pub session_idle_minutes: u32,
    /// A session ends this many hours after its login, used or not.
    pub session_max_hours: u32,
    /// A re-authentication is valid for sensitive actions for this many
    /// minutes.
    pub reauth_minutes: u32,
    /// Entries of the remote-access security log are kept this many days
    /// (1–3650; purged daily). M2b follow-up.
    pub audit_retention_days: u32,
}

impl Default for RemoteUiSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_address: "127.0.0.1".to_owned(),
            port: DEFAULT_REMOTE_UI_PORT,
            host_name: None,
            tls: RemoteTlsMode::SelfSigned,
            acme: AcmeSettings::default(),
            internet_mode: false,
            allowed_networks: Vec::new(),
            session_idle_minutes: 30,
            session_max_hours: 12,
            reauth_minutes: 5,
            audit_retention_days: DEFAULT_AUDIT_RETENTION_DAYS,
        }
    }
}

/// Default of [`RemoteUiSettings::audit_retention_days`].
pub const DEFAULT_AUDIT_RETENTION_DAYS: u32 = 90;
/// Largest [`RemoteUiSettings::audit_retention_days`] (ten years).
pub const MAX_AUDIT_RETENTION_DAYS: u32 = 3650;

/// Something wrong with [`RemoteUiSettings`]; the core answers
/// `invalid_request` (or `forbidden` for internet mode) and names it in
/// the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteUiSettingsIssue {
    /// `bind_address` is not an IP address.
    InvalidBindAddress,
    /// `port` is 0.
    InvalidPort,
    /// A non-loopback `bind_address` with TLS off.
    TlsRequired,
    /// Internet mode is locked ([`internet_mode_lock`]).
    InternetModeLocked,
    /// ACME needs internet mode.
    AcmeNeedsInternetMode,
    /// ACME needs at least one domain and accepted terms.
    AcmeIncomplete,
    /// An entry of `allowed_networks` is not a CIDR network.
    InvalidNetwork,
    /// An entry of `allowed_networks` reaches beyond local networks
    /// ([`is_local_network`]) while internet mode is locked.
    PublicNetwork,
    /// `host_name` is not a DNS name.
    InvalidHostName,
    /// A session limit is 0, or the idle limit exceeds the maximum age.
    InvalidSessionLimits,
    /// `audit_retention_days` is 0 or more than
    /// [`MAX_AUDIT_RETENTION_DAYS`].
    InvalidAuditRetention,
}

impl RemoteUiSettingsIssue {
    /// Error code the core answers with for this issue.
    pub fn error_code(self) -> ErrorCode {
        match self {
            Self::InternetModeLocked | Self::AcmeNeedsInternetMode | Self::PublicNetwork => {
                ErrorCode::Forbidden
            }
            _ => ErrorCode::InvalidRequest,
        }
    }
}

impl RemoteUiSettings {
    /// The parsed bind address, `None` when it is not an IP address.
    pub fn bind_ip(&self) -> Option<IpAddr> {
        self.bind_address.trim().parse().ok()
    }

    /// Whether the remote UI would listen on loopback only.
    pub fn is_loopback(&self) -> bool {
        self.bind_ip().is_some_and(|ip| ip.is_loopback())
    }

    /// Everything wrong with these settings (empty = valid), checked
    /// against the current internet-mode lock.
    pub fn issues(&self) -> Vec<RemoteUiSettingsIssue> {
        use RemoteUiSettingsIssue as I;
        let mut issues = Vec::new();
        match self.bind_ip() {
            None => issues.push(I::InvalidBindAddress),
            Some(ip) if !ip.is_loopback() && self.tls == RemoteTlsMode::Off => {
                issues.push(I::TlsRequired);
            }
            Some(_) => {}
        }
        if self.port == 0 {
            issues.push(I::InvalidPort);
        }
        if self.internet_mode && internet_mode_lock().is_some() {
            issues.push(I::InternetModeLocked);
        }
        if self.tls == RemoteTlsMode::Acme {
            if !self.internet_mode {
                issues.push(I::AcmeNeedsInternetMode);
            }
            if self.acme.domains.is_empty() || !self.acme.accept_terms {
                issues.push(I::AcmeIncomplete);
            }
        }
        if !self.allowed_networks.iter().all(|n| is_cidr(n)) {
            issues.push(I::InvalidNetwork);
        } else if (!self.internet_mode || internet_mode_lock().is_some())
            && !self.allowed_networks.iter().all(|n| is_local_network(n))
        {
            issues.push(I::PublicNetwork);
        }
        if self.host_name.as_deref().is_some_and(|h| !is_dns_name(h)) {
            issues.push(I::InvalidHostName);
        }
        if self.session_idle_minutes == 0
            || self.session_max_hours == 0
            || self.reauth_minutes == 0
            || u64::from(self.session_idle_minutes) > u64::from(self.session_max_hours) * 60
        {
            issues.push(I::InvalidSessionLimits);
        }
        if self.audit_retention_days == 0 || self.audit_retention_days > MAX_AUDIT_RETENTION_DAYS {
            issues.push(I::InvalidAuditRetention);
        }
        issues
    }
}

/// Whether `text` is `<ip>/<prefix>` with a prefix that fits the address.
fn is_cidr(text: &str) -> bool {
    let Some((ip, prefix)) = text.trim().split_once('/') else {
        return false;
    };
    let (Ok(ip), Ok(prefix)) = (ip.parse::<IpAddr>(), prefix.parse::<u8>()) else {
        return false;
    };
    prefix <= if ip.is_ipv4() { 32 } else { 128 }
}

/// Local address ranges: loopback, RFC 1918, link-local, CGNAT
/// (RFC 6598), IPv6 unique local (`fc00::/7`) and link-local (`fe80::/10`).
/// Outside internet mode only peers from these ranges may connect.
const LOCAL_V4: [([u8; 4], u8); 6] = [
    ([127, 0, 0, 0], 8),
    ([10, 0, 0, 0], 8),
    ([172, 16, 0, 0], 12),
    ([192, 168, 0, 0], 16),
    ([169, 254, 0, 0], 16),
    ([100, 64, 0, 0], 10),
];
const LOCAL_V6: [(u128, u8); 3] = [(1, 128), (0xfc00_u128 << 112, 7), (0xfe80_u128 << 112, 10)];

/// [`LOCAL_V4`] and [`LOCAL_V6`] as CIDR text (for the generated TypeScript
/// constants).
pub(crate) fn local_networks() -> Vec<String> {
    let v4 = LOCAL_V4
        .iter()
        .map(|(base, len)| format!("{}/{len}", std::net::Ipv4Addr::from(*base)));
    let v6 = LOCAL_V6
        .iter()
        .map(|(base, len)| format!("{}/{len}", std::net::Ipv6Addr::from(*base)));
    v4.chain(v6).collect()
}

fn v4_in(ip: u32, prefix: u8, (base, len): ([u8; 4], u8)) -> bool {
    let mask = if len == 0 { 0 } else { u32::MAX << (32 - len) };
    prefix >= len && ip & mask == u32::from_be_bytes(base)
}

fn v6_in(ip: u128, prefix: u8, (base, len): (u128, u8)) -> bool {
    let mask = if len == 0 {
        0
    } else {
        u128::MAX << (128 - len)
    };
    prefix >= len && ip & mask == base
}

/// Whether the network `ip/prefix` lies entirely within the local ranges.
/// IPv4-mapped IPv6 addresses count as their IPv4 address.
fn local_net(ip: IpAddr, prefix: u8) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let ip = u32::from(v4);
            LOCAL_V4.iter().any(|&range| v4_in(ip, prefix, range))
        }
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) if prefix >= 96 => local_net(IpAddr::V4(v4), prefix - 96),
            _ => {
                let ip = u128::from(v6);
                LOCAL_V6.iter().any(|&range| v6_in(ip, prefix, range))
            }
        },
    }
}

/// Whether `ip` is a local address (loopback, private, link-local, CGNAT,
/// IPv6 unique local): the only peers the remote UI accepts while
/// internet mode is off.
pub fn is_local_address(ip: IpAddr) -> bool {
    local_net(ip, if ip.is_ipv4() { 32 } else { 128 })
}

/// Whether the CIDR network `text` lies entirely within local ranges
/// ([`is_local_address`]); `false` for text that is not a CIDR network.
pub fn is_local_network(text: &str) -> bool {
    if !is_cidr(text) {
        return false;
    }
    let Some((ip, prefix)) = text.trim().split_once('/') else {
        return false;
    };
    match (ip.parse::<IpAddr>(), prefix.parse::<u8>()) {
        (Ok(ip), Ok(prefix)) => local_net(ip, prefix),
        _ => false,
    }
}

/// A conservative DNS host name check ([`crate::hosts::is_dns_name`]); a
/// trailing dot is allowed.
fn is_dns_name(name: &str) -> bool {
    let name = name.strip_suffix('.').unwrap_or(name);
    name.parse::<IpAddr>().is_err() && crate::hosts::is_dns_name(name)
}

/// Why internet mode is locked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum InternetModeLockReason {
    /// The external penetration test (the M2 human gate) has not been
    /// recorded as passed.
    PentestPending,
}

/// The internet-mode lock of this build: `Some(reason)` while locked. In
/// M2 it is always locked; a later release flips this after the gate.
pub const fn internet_mode_lock() -> Option<InternetModeLockReason> {
    Some(InternetModeLockReason::PentestPending)
}

/// Where the served certificate came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum CertificateSource {
    /// Made by Nail.
    SelfSigned,
    /// Issued by an ACME CA.
    Acme,
}

/// The certificate the remote UI serves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RemoteCertificateInfo {
    /// Self-signed or ACME.
    pub source: CertificateSource,
    /// SHA-256 of the DER certificate, uppercase hexadecimal without
    /// separators (shown for pinning at pairing; the UI groups it).
    pub fingerprint_sha256: String,
    /// DNS names and IP addresses in the certificate.
    pub subject_names: Vec<String>,
    /// Valid from (RFC 3339).
    pub not_before: String,
    /// Valid until (RFC 3339).
    pub not_after: String,
}

/// Whether the remote UI runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteUiState {
    /// Not serving (disabled, or not started yet).
    #[default]
    Stopped,
    /// Starting (binding, making or fetching the certificate).
    Starting,
    /// Serving at `urls`.
    Running,
    /// Could not start; see `error` / `message`.
    Failed,
}

/// The remote UI's current state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RemoteUiStatus {
    /// Running or not.
    pub state: RemoteUiState,
    /// Addresses browsers can use (`https://host:port/`), empty when not
    /// running.
    pub urls: Vec<String>,
    /// The served certificate, `null` without TLS or when not running.
    pub certificate: Option<RemoteCertificateInfo>,
    /// Whether internet mode is locked (always `true` in M2).
    pub internet_mode_locked: bool,
    /// Why it is locked, `null` when it is not.
    pub internet_mode_lock_reason: Option<InternetModeLockReason>,
    /// Sessions that are currently valid.
    pub active_sessions: u32,
    /// Why starting failed (`state` = `failed`), else `null`.
    pub error: Option<ErrorCode>,
    /// Diagnostic detail (English), else `null`.
    pub message: Option<String>,
    /// The user-facing text (Fluent id + arguments) when the core has a more
    /// specific one than the error class; else `null` (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
}

impl RemoteUiStatus {
    /// Not running, with this build's internet-mode lock.
    pub fn stopped() -> Self {
        let lock = internet_mode_lock();
        Self {
            state: RemoteUiState::Stopped,
            urls: Vec::new(),
            certificate: None,
            internet_mode_locked: lock.is_some(),
            internet_mode_lock_reason: lock,
            active_sessions: 0,
            error: None,
            message: None,
            localized: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Grants
// ---------------------------------------------------------------------------

/// What a remote session may do: the same scopes, accounts and folder
/// exclusions as an MCP client ([`McpGrant`]; `skip_draft_confirmation`
/// has no meaning here), plus `admin` for administration (settings,
/// accounts, keys, remote users, grants), which also needs a fresh
/// re-authentication. A session never holds more than its device, and a
/// device never more than its user ([`RemoteGrant::intersect`]).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct RemoteGrant {
    /// Mail and PIM access.
    pub mail: McpGrant,
    /// Administration.
    pub admin: bool,
}

/// The presets offered when pairing (design: read-only, no-send, full).
/// None of them includes decrypted content (`encrypted`), the desktop's UI
/// context (`context`) or `admin`: those are explicit extra choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteAccessPreset {
    /// Read mail, attachments and sources.
    ReadOnly,
    /// Everything but sending.
    NoSend,
    /// Everything (without the explicit extras above).
    Full,
}

impl RemoteAccessPreset {
    /// All presets, narrowest first.
    pub const ALL: [RemoteAccessPreset; 3] = [Self::ReadOnly, Self::NoSend, Self::Full];

    /// The preset's grant (all accounts).
    pub fn grant(self) -> RemoteGrant {
        use McpScope as S;
        let read = [S::Read, S::RawSource, S::Attachments];
        let write = [
            S::Organize,
            S::PermanentDelete,
            S::Draft,
            S::Calendar,
            S::Pim,
            S::Rules,
        ];
        let mut scopes: Vec<McpScope> = read.to_vec();
        if self != Self::ReadOnly {
            scopes.extend(write);
        }
        if self == Self::Full {
            scopes.push(S::Send);
        }
        scopes.sort_by_key(|s| McpScope::ALL.iter().position(|x| x == s));
        RemoteGrant {
            mail: McpGrant {
                scopes,
                all_accounts: true,
                ..McpGrant::default()
            },
            admin: false,
        }
    }
}

impl RemoteGrant {
    /// What both grants allow: common scopes; accounts both reach; the
    /// folder exclusions of either; `admin` only when both have it.
    #[must_use]
    pub fn intersect(&self, other: &RemoteGrant) -> RemoteGrant {
        let (a, b) = (&self.mail, &other.mail);
        let scopes = a
            .scopes
            .iter()
            .copied()
            .filter(|s| b.scopes.contains(s))
            .collect();
        let (all_accounts, accounts) = match (a.all_accounts, b.all_accounts) {
            (true, true) => (true, Vec::new()),
            (true, false) => (false, b.accounts.clone()),
            (false, true) => (false, a.accounts.clone()),
            (false, false) => (
                false,
                a.accounts
                    .iter()
                    .filter(|x| b.accounts.contains(x))
                    .cloned()
                    .collect(),
            ),
        };
        let mut excluded_folders = a.excluded_folders.clone();
        for folder in &b.excluded_folders {
            if !excluded_folders.contains(folder) {
                excluded_folders.push(folder.clone());
            }
        }
        RemoteGrant {
            mail: McpGrant {
                scopes,
                all_accounts,
                accounts,
                excluded_folders,
                skip_draft_confirmation: false,
            },
            admin: self.admin && other.admin,
        }
    }

    /// Whether `self` allows nothing beyond `ceiling`.
    pub fn is_within(&self, ceiling: &RemoteGrant) -> bool {
        self.intersect(ceiling).normalized() == self.clone().normalized()
    }

    fn normalized(mut self) -> Self {
        self.mail
            .scopes
            .sort_by_key(|s| McpScope::ALL.iter().position(|x| x == s));
        self.mail.scopes.dedup();
        self.mail.accounts.sort();
        self.mail.accounts.dedup();
        self.mail.excluded_folders.sort();
        self.mail.excluded_folders.dedup();
        if self.mail.all_accounts {
            self.mail.accounts.clear();
        }
        self.mail.skip_draft_confirmation = false;
        self
    }
}

// ---------------------------------------------------------------------------
// Users, credentials, devices, sessions, pairing
// ---------------------------------------------------------------------------

/// A registered passkey (WebAuthn credential). The public key and
/// signature counter stay in the core.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Passkey {
    /// Opaque id.
    pub id: PasskeyId,
    /// Name given by the user (e.g. "Phone").
    pub name: String,
    /// Registration time (RFC 3339).
    pub created_at: String,
    /// Last login with it (RFC 3339), `null` = never.
    pub last_used_at: Option<String>,
    /// The device it was registered from, if known.
    pub device_id: Option<RemoteDeviceId>,
}

/// A remote-UI user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RemoteUser {
    /// Opaque id.
    pub id: RemoteUserId,
    /// Login name (unique, case-insensitive).
    pub name: String,
    /// Display name, `null` = the login name.
    pub display_name: Option<String>,
    /// Creation time (RFC 3339).
    pub created_at: String,
    /// Last successful login (RFC 3339), `null` = never.
    pub last_login_at: Option<String>,
    /// A password is set.
    pub has_password: bool,
    /// TOTP is enrolled and confirmed (then required with the password).
    pub totp_enabled: bool,
    /// Registered passkeys, oldest first.
    pub passkeys: Vec<Passkey>,
    /// The most this user's devices and sessions may be granted.
    pub grant: RemoteGrant,
    /// Disabled users cannot log in; their sessions are ended.
    pub disabled: bool,
    /// Locked out after failed logins until this time (RFC 3339), `null`
    /// = not locked.
    pub locked_until: Option<String>,
}

/// Changes to a [`RemoteUser`]; `null` fields stay as they are.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct RemoteUserUpdate {
    /// New display name (`""` clears it).
    pub display_name: Option<String>,
    /// New grant ceiling (devices and sessions above it are narrowed).
    pub grant: Option<RemoteGrant>,
    /// Disable or enable.
    pub disabled: Option<bool>,
}

/// A browser paired with the remote UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RemoteDevice {
    /// Opaque id.
    pub id: RemoteDeviceId,
    /// The user it belongs to.
    pub user_id: RemoteUserId,
    /// Name (from the pairing code, else what the browser sent).
    pub name: String,
    /// Pairing time (RFC 3339).
    pub paired_at: String,
    /// Last request (RFC 3339), `null` = none since pairing.
    pub last_seen_at: Option<String>,
    /// Browser `User-Agent` at the last request.
    pub user_agent: Option<String>,
    /// Client address at the last request.
    pub last_ip: Option<String>,
    /// The most its sessions may be granted (within the user's grant).
    pub grant: RemoteGrant,
    /// When it was revoked (RFC 3339); a revoked device must pair again.
    pub revoked_at: Option<String>,
}

/// How a session was authenticated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteAuthMethod {
    /// A passkey (WebAuthn).
    Passkey,
    /// Password alone (only for users without TOTP).
    Password,
    /// Password and TOTP code.
    PasswordTotp,
    /// A pairing code (the first session of a newly paired device).
    Pairing,
}

/// Why a session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum SessionEndReason {
    /// The user logged out.
    Logout,
    /// Revoked from the session list.
    Revoked,
    /// Idle or maximum age reached.
    Expired,
    /// Its device was revoked.
    DeviceRevoked,
    /// Its user was disabled or removed, or its credentials changed.
    UserChanged,
}

/// A remote-UI session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RemoteSession {
    /// Opaque id (not the token).
    pub id: RemoteSessionId,
    /// The user.
    pub user_id: RemoteUserId,
    /// The user's login name.
    pub user_name: String,
    /// The device.
    pub device_id: RemoteDeviceId,
    /// The device's name.
    pub device_name: String,
    /// How it logged in.
    pub auth_method: RemoteAuthMethod,
    /// Login time (RFC 3339).
    pub created_at: String,
    /// Last request (RFC 3339).
    pub last_seen_at: String,
    /// Ends at the latest at (RFC 3339; the idle limit may end it sooner).
    pub expires_at: String,
    /// Client address at the last request.
    pub ip: Option<String>,
    /// Browser `User-Agent`.
    pub user_agent: Option<String>,
    /// Last re-authentication (RFC 3339), `null` = none since login (the
    /// login itself counts for [`RemoteUiSettings::reauth_minutes`]).
    pub reauth_at: Option<String>,
    /// What it may do.
    pub grant: RemoteGrant,
    /// When it ended (RFC 3339), `null` while valid.
    pub ended_at: Option<String>,
    /// Why it ended, `null` while valid.
    pub end_reason: Option<SessionEndReason>,
}

/// Which sessions [`crate::Request::ListRemoteSessions`] returns.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct RemoteSessionFilter {
    /// Only this user's.
    pub user_id: Option<RemoteUserId>,
    /// Only this device's.
    pub device_id: Option<RemoteDeviceId>,
    /// Also ended sessions (newest first, bounded by the core).
    pub include_ended: bool,
}

/// State of a pairing code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum PairingState {
    /// Waiting to be entered in a browser.
    Pending,
    /// Used: a device was paired.
    Used,
    /// Not used in time, or too many wrong attempts.
    Expired,
    /// Cancelled on the desktop.
    Cancelled,
}

/// A one-time pairing code. Creating it on the desktop approves the
/// device that enters it; it is single-use and short-lived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PairingCode {
    /// Opaque id (not the code).
    pub id: PairingId,
    /// The user the new device will belong to.
    pub user_id: RemoteUserId,
    /// Name for the new device, `null` = what the browser sends.
    pub device_name: Option<String>,
    /// Grant of the new device (within the user's).
    pub grant: RemoteGrant,
    /// Creation time (RFC 3339).
    pub created_at: String,
    /// Valid until (RFC 3339).
    pub expires_at: String,
    /// State.
    pub state: PairingState,
    /// The device paired with it (`state` = `used`).
    pub device_id: Option<RemoteDeviceId>,
}

/// A TOTP secret to enroll (RFC 6238: HMAC-SHA1, 6 digits, 30 s). Shown
/// once; confirmed with [`crate::Request::ConfirmTotpEnrollment`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TotpEnrollment {
    /// The user.
    pub user_id: RemoteUserId,
    /// The secret, base32 without padding (for manual entry).
    pub secret: String,
    /// `otpauth://totp/…` URI (for a QR code).
    pub otpauth_uri: String,
    /// Issuer shown by authenticator apps (`Nail`).
    pub issuer: String,
    /// Account name shown by authenticator apps.
    pub account_name: String,
    /// Code length.
    pub digits: u32,
    /// Time step in seconds.
    pub period_secs: u32,
}

// ---------------------------------------------------------------------------
// Audit and alerts
// ---------------------------------------------------------------------------

/// What an audit entry records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteAuditKind {
    /// A login (successful or not; see `outcome`).
    Login,
    /// A re-authentication for a sensitive action.
    Reauth,
    /// A logout.
    Logout,
    /// A pairing code was created.
    PairingCreated,
    /// A pairing code was entered (a device paired, or a wrong code).
    Pairing,
    /// Login attempts were throttled or an account / address locked out.
    Lockout,
    /// A password was set, changed or removed.
    PasswordChanged,
    /// TOTP was enabled or disabled.
    TotpChanged,
    /// A passkey was registered, renamed or removed.
    PasskeyChanged,
    /// A session was revoked or expired.
    SessionEnded,
    /// A device was revoked or changed.
    DeviceChanged,
    /// A user was created, changed or removed.
    UserChanged,
    /// The remote-UI settings changed (or a change was refused).
    SettingsChanged,
    /// A request of a remote session (`name` = the request type); denied
    /// ones are always recorded, allowed writes too.
    Request,
}

/// How an audited event ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteAuditOutcome {
    /// Succeeded.
    Success,
    /// Failed (wrong credentials, expired code, …).
    Failure,
    /// Refused (not granted, locked out, rate limited, re-authentication
    /// needed).
    Denied,
}

/// One entry of the remote-UI audit log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RemoteAuditEntry {
    /// Monotonic id (newest is highest); page with
    /// [`crate::Request::ListRemoteAudit`]'s `before`.
    pub id: u64,
    /// Time (RFC 3339).
    pub at: String,
    /// What happened.
    pub kind: RemoteAuditKind,
    /// How it ended.
    pub outcome: RemoteAuditOutcome,
    /// The user, when known.
    pub user_id: Option<RemoteUserId>,
    /// The login name (also the name tried in a failed login).
    pub user_name: Option<String>,
    /// The device, when known.
    pub device_id: Option<RemoteDeviceId>,
    /// The session, when known.
    pub session_id: Option<RemoteSessionId>,
    /// Client address.
    pub ip: Option<String>,
    /// Browser `User-Agent`.
    pub user_agent: Option<String>,
    /// Login method, for logins and re-authentications.
    pub method: Option<RemoteAuthMethod>,
    /// Request type or other short name (e.g. the passkey's name).
    pub name: Option<String>,
    /// Error class of a failure or denial.
    pub error: Option<ErrorCode>,
    /// Diagnostic detail (English), else `null`.
    pub detail: Option<String>,
    /// The user-facing text (Fluent id + arguments) when the core has a more
    /// specific one than the error class; else `null` (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
}

/// A security alert for the desktop (and every admin session): shown as a
/// notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RemoteAlert {
    /// A new device was paired.
    NewDevice {
        /// The device.
        device_id: RemoteDeviceId,
        /// Its name.
        device_name: String,
        /// Its user's login name.
        user_name: String,
        /// Where it connected from.
        ip: Option<String>,
    },
    /// Failed logins locked out a user or an address.
    Lockout {
        /// The login name tried, if any.
        user_name: Option<String>,
        /// The address, if the address was locked out.
        ip: Option<String>,
        /// Locked until (RFC 3339).
        until: String,
    },
}

// ---------------------------------------------------------------------------
// Login protocol (POST AUTH_PATH)
// ---------------------------------------------------------------------------

/// Why a login step is taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AuthPurpose {
    /// Start a session.
    #[default]
    Login,
    /// Confirm the user of the current session again before a sensitive
    /// action (sets [`RemoteSession::reauth_at`]).
    Reauth,
}

/// A request of the login protocol: `POST` [`AUTH_PATH`] with this JSON
/// (tagged by `type`) and the [`CSRF_HEADER`] (except `Status`, which
/// hands out the token). Answered with an [`AuthResponse`]; HTTP status
/// 200 for every protocol-level answer, including `Failed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum AuthRequest {
    /// Whether this browser is logged in, which methods exist, and a CSRF
    /// token. Answered with [`AuthResponse::Status`].
    Status,
    /// Pair this browser with a code shown on the desktop. Sets the device
    /// cookie and starts a session ([`RemoteAuthMethod::Pairing`]).
    /// Answered with [`AuthResponse::LoggedIn`] or `Failed`.
    Pair {
        /// The code as typed (case and separators are ignored).
        code: String,
        /// Name for the device, used when the code has none.
        device_name: String,
    },
    /// Log in (or re-authenticate) with a password, plus the TOTP code
    /// when the user has TOTP. Answered with `LoggedIn` (or
    /// `Reauthenticated`), `TotpRequired` or `Failed`.
    Password {
        /// Login name.
        user_name: String,
        /// Password.
        password: String,
        /// TOTP code, `null` on the first try.
        #[serde(default)]
        totp_code: Option<String>,
        /// Login or re-authentication.
        #[serde(default)]
        purpose: AuthPurpose,
    },
    /// Start a passkey login: get a WebAuthn challenge. Answered with
    /// [`AuthResponse::PasskeyChallenge`].
    PasskeyStart {
        /// Login name, `null` for a discoverable credential.
        #[serde(default)]
        user_name: Option<String>,
        /// Login or re-authentication.
        #[serde(default)]
        purpose: AuthPurpose,
    },
    /// Finish a passkey login with the authenticator's answer. Answered
    /// with `LoggedIn` (or `Reauthenticated`) or `Failed`.
    PasskeyFinish {
        /// From the challenge.
        challenge_id: String,
        /// `PublicKeyCredential.toJSON()` of the assertion.
        #[ts(type = "unknown")]
        credential: serde_json::Value,
    },
    /// Start registering a passkey for the session's user (needs a fresh
    /// re-authentication, or the first minutes after pairing). Answered
    /// with [`AuthResponse::PasskeyChallenge`].
    PasskeyRegisterStart {
        /// Name for the passkey.
        name: String,
    },
    /// Finish registering a passkey. Answered with
    /// [`AuthResponse::PasskeyRegistered`] or `Failed`.
    PasskeyRegisterFinish {
        /// From the challenge.
        challenge_id: String,
        /// `PublicKeyCredential.toJSON()` of the attestation.
        #[ts(type = "unknown")]
        credential: serde_json::Value,
    },
    /// End the session (the device stays paired). Answered with
    /// [`AuthResponse::LoggedOut`].
    Logout,
}

/// Why a login step failed. Deliberately coarse: a wrong name and a wrong
/// password look the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AuthFailure {
    /// Wrong name, password, TOTP code or passkey.
    InvalidCredentials,
    /// Wrong, used, expired or cancelled pairing code.
    InvalidCode,
    /// The challenge is unknown or expired.
    ChallengeExpired,
    /// Too many attempts; try again after `retry_after_secs`.
    RateLimited,
    /// Locked out after failed logins (the desktop was notified).
    LockedOut,
    /// This browser is not paired (or its device was revoked): pair first.
    NotPaired,
    /// No session (for steps that need one).
    NotLoggedIn,
    /// The step needs a fresh re-authentication.
    ReauthRequired,
    /// The user is disabled.
    Disabled,
    /// The request is malformed or the origin / CSRF token is wrong.
    BadRequest,
}

/// Login methods the server offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct LoginMethods {
    /// Passkeys (needs a host name as relying-party id).
    pub passkey: bool,
    /// Password (+ TOTP).
    pub password: bool,
}

/// The browser's login state ([`AuthRequest::Status`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AuthStatus {
    /// Whether this browser has a valid session.
    pub authenticated: bool,
    /// Whether this browser is a paired device.
    pub paired: bool,
    /// The session, when logged in.
    pub session: Option<RemoteSession>,
    /// Re-authentication valid until (RFC 3339), `null` = not fresh.
    pub reauth_until: Option<String>,
    /// Token for the [`CSRF_HEADER`].
    pub csrf_token: String,
    /// Offered login methods.
    pub methods: LoginMethods,
    /// Fingerprint of the served certificate (the browser shows it to
    /// compare with the desktop at pairing), `null` without TLS.
    pub certificate_fingerprint: Option<String>,
}

/// The answer to an [`AuthRequest`] (tagged by `type`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum AuthResponse {
    /// Answer to [`AuthRequest::Status`].
    Status {
        /// The state.
        status: AuthStatus,
    },
    /// A session started (the session cookie is set).
    LoggedIn {
        /// The session.
        session: RemoteSession,
    },
    /// The re-authentication succeeded.
    Reauthenticated {
        /// Valid until (RFC 3339).
        reauth_until: String,
    },
    /// The password was right; send it again with the TOTP code.
    TotpRequired,
    /// A WebAuthn challenge: pass `options` to
    /// `navigator.credentials.get()` / `.create()` (after
    /// `PublicKeyCredential.parse{Request,Creation}OptionsFromJSON`).
    PasskeyChallenge {
        /// Echo in the finish step.
        challenge_id: String,
        /// `PublicKeyCredentialRequestOptionsJSON` or
        /// `PublicKeyCredentialCreationOptionsJSON`.
        #[ts(type = "unknown")]
        options: serde_json::Value,
    },
    /// A passkey was registered.
    PasskeyRegistered {
        /// The passkey.
        passkey: Passkey,
    },
    /// The session ended (the cookie is cleared).
    LoggedOut,
    /// The step failed.
    Failed {
        /// Why.
        reason: AuthFailure,
        /// Seconds until another attempt is accepted, when throttled.
        retry_after_secs: Option<u32>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{AccountId, FolderId};
    use serde_json::json;

    #[test]
    fn defaults_are_off_on_loopback_with_tls() {
        let settings: RemoteUiSettings = serde_json::from_value(json!({})).unwrap();
        assert_eq!(settings, RemoteUiSettings::default());
        assert!(!settings.enabled && !settings.internet_mode);
        assert!(settings.is_loopback());
        assert_eq!(settings.tls, RemoteTlsMode::SelfSigned);
        assert_eq!(settings.port, DEFAULT_REMOTE_UI_PORT);
        assert_eq!(settings.issues().len(), 0);
        let value = serde_json::to_value(&settings).unwrap();
        assert_eq!(value["tls"], json!("self-signed"));
        assert_eq!(
            value["acme"]["directory_url"],
            json!(LETS_ENCRYPT_DIRECTORY)
        );
    }

    #[test]
    fn local_addresses_and_networks() {
        let local = |ip: &str| is_local_address(ip.parse().unwrap());
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.20",
            "169.254.3.4",
            "100.64.0.1",
            "100.127.255.254",
            "::1",
            "fd12:3456::1",
            "fc00::1",
            "fe80::1",
            "::ffff:192.168.1.20",
        ] {
            assert!(local(ip), "{ip}");
        }
        for ip in [
            "8.8.8.8",
            "172.32.0.1",
            "100.128.0.1",
            "192.169.0.1",
            "2001:db8::1",
            "2a00:1450::1",
            "::ffff:8.8.8.8",
            "::",
            "0.0.0.0",
        ] {
            assert!(!local(ip), "{ip}");
        }
        assert!(is_local_network("192.168.1.0/24"));
        assert!(is_local_network("10.0.0.0/8"));
        assert!(is_local_network("fd00::/8"));
        assert!(is_local_network("::ffff:192.168.0.0/112"));
        // Wider than the local range, or public.
        assert!(!is_local_network("10.0.0.0/7"));
        assert!(!is_local_network("0.0.0.0/0"));
        assert!(!is_local_network("::/0"));
        assert!(!is_local_network("203.0.113.0/24"));
        assert!(!is_local_network("not a network"));
    }

    #[test]
    fn public_allowed_networks_are_refused_while_internet_mode_is_locked() {
        use RemoteUiSettingsIssue as I;
        let lan = RemoteUiSettings {
            bind_address: "0.0.0.0".into(),
            allowed_networks: vec!["192.168.1.0/24".into(), "fd00::/8".into()],
            ..RemoteUiSettings::default()
        };
        assert_eq!(lan.issues().len(), 0);
        let public = RemoteUiSettings {
            allowed_networks: vec!["192.168.1.0/24".into(), "0.0.0.0/0".into()],
            ..lan
        };
        assert_eq!(public.issues(), [I::PublicNetwork]);
        assert_eq!(I::PublicNetwork.error_code(), ErrorCode::Forbidden);
        assert_eq!(
            serde_json::to_value(I::PublicNetwork).unwrap(),
            json!("public-network")
        );
    }

    #[test]
    fn settings_validation() {
        use RemoteUiSettingsIssue as I;
        let lan = RemoteUiSettings {
            enabled: true,
            bind_address: "0.0.0.0".into(),
            tls: RemoteTlsMode::Off,
            ..RemoteUiSettings::default()
        };
        assert_eq!(lan.issues(), [I::TlsRequired]);
        let lan = RemoteUiSettings {
            tls: RemoteTlsMode::SelfSigned,
            ..lan
        };
        assert_eq!(lan.issues().len(), 0);
        let local_plain = RemoteUiSettings {
            bind_address: "::1".into(),
            tls: RemoteTlsMode::Off,
            ..RemoteUiSettings::default()
        };
        assert_eq!(local_plain.issues().len(), 0);

        let internet = RemoteUiSettings {
            internet_mode: true,
            tls: RemoteTlsMode::Acme,
            acme: AcmeSettings {
                domains: vec!["mail.example.org".into()],
                accept_terms: true,
                ..AcmeSettings::default()
            },
            ..RemoteUiSettings::default()
        };
        assert_eq!(internet.issues(), [I::InternetModeLocked]);
        assert_eq!(I::InternetModeLocked.error_code(), ErrorCode::Forbidden);
        let acme_on_lan = RemoteUiSettings {
            tls: RemoteTlsMode::Acme,
            ..RemoteUiSettings::default()
        };
        assert_eq!(
            acme_on_lan.issues(),
            [I::AcmeNeedsInternetMode, I::AcmeIncomplete]
        );

        let broken = RemoteUiSettings {
            bind_address: "nail.local".into(),
            port: 0,
            host_name: Some("192.168.1.2".into()),
            allowed_networks: vec!["10.0.0.0/8".into(), "10.0.0.0/33".into()],
            session_idle_minutes: 24 * 60,
            session_max_hours: 12,
            audit_retention_days: 0,
            ..RemoteUiSettings::default()
        };
        assert_eq!(
            broken.issues(),
            [
                I::InvalidBindAddress,
                I::InvalidPort,
                I::InvalidNetwork,
                I::InvalidHostName,
                I::InvalidSessionLimits,
                I::InvalidAuditRetention
            ]
        );
        assert_eq!(I::InvalidPort.error_code(), ErrorCode::InvalidRequest);
    }

    #[test]
    fn host_names_and_networks() {
        assert!(is_dns_name("nail.local"));
        assert!(is_dns_name("desktop-7"));
        assert!(is_dns_name("mail.example.org."));
        assert!(!is_dns_name("-bad.example"));
        assert!(!is_dns_name("under_score.example"));
        assert!(!is_dns_name("10.0.0.1"));
        assert!(!is_dns_name("host.123"));
        assert!(!is_dns_name(""));
        assert!(is_cidr("fd00::/8"));
        assert!(is_cidr("192.168.0.0/16"));
        assert!(!is_cidr("192.168.0.0"));
        assert!(!is_cidr("fd00::/129"));
    }

    #[test]
    fn internet_mode_is_locked_in_m2() {
        assert_eq!(
            internet_mode_lock(),
            Some(InternetModeLockReason::PentestPending)
        );
        let status = RemoteUiStatus::stopped();
        assert!(status.internet_mode_locked);
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(value["state"], json!("stopped"));
        assert_eq!(value["internet_mode_lock_reason"], json!("pentest-pending"));
    }

    #[test]
    fn presets_never_include_the_explicit_extras() {
        for preset in RemoteAccessPreset::ALL {
            let grant = preset.grant();
            assert!(!grant.admin);
            assert!(grant.mail.all_accounts);
            assert!(grant.mail.has(McpScope::Read));
            assert!(!grant.mail.has(McpScope::Encrypted));
            assert!(!grant.mail.has(McpScope::Context));
        }
        assert!(!RemoteAccessPreset::NoSend.grant().mail.has(McpScope::Send));
        assert!(RemoteAccessPreset::Full.grant().mail.has(McpScope::Send));
        assert!(
            !RemoteAccessPreset::ReadOnly
                .grant()
                .mail
                .has(McpScope::Organize)
        );
        assert!(
            RemoteAccessPreset::ReadOnly
                .grant()
                .is_within(&RemoteAccessPreset::Full.grant())
        );
        assert!(
            !RemoteAccessPreset::Full
                .grant()
                .is_within(&RemoteAccessPreset::NoSend.grant())
        );
        assert_eq!(
            serde_json::to_value(RemoteAccessPreset::NoSend).unwrap(),
            json!("no-send")
        );
    }

    #[test]
    fn grants_intersect() {
        let user = RemoteGrant {
            mail: McpGrant {
                scopes: vec![McpScope::Read, McpScope::Send],
                all_accounts: false,
                accounts: vec!["work".into(), "home".into()],
                excluded_folders: vec!["f-hr".into()],
                skip_draft_confirmation: true,
            },
            admin: true,
        };
        let device = RemoteGrant {
            mail: McpGrant {
                scopes: vec![McpScope::Read, McpScope::Organize],
                all_accounts: true,
                excluded_folders: vec!["f-private".into()],
                ..McpGrant::default()
            },
            admin: false,
        };
        let both = user.intersect(&device);
        assert_eq!(both.mail.scopes, [McpScope::Read]);
        assert!(!both.mail.all_accounts);
        assert_eq!(
            both.mail.accounts,
            [AccountId::from("work"), AccountId::from("home")]
        );
        assert_eq!(
            both.mail.excluded_folders,
            [FolderId::from("f-hr"), FolderId::from("f-private")]
        );
        assert!(!both.admin && !both.mail.skip_draft_confirmation);
        assert!(both.is_within(&user) && both.is_within(&device));
        assert!(!user.is_within(&device));
        let none = RemoteGrant::default();
        assert!(none.mail.is_empty());
        assert!(none.is_within(&none));
    }

    #[test]
    fn auth_protocol_is_type_tagged() {
        let pair: AuthRequest = serde_json::from_value(json!({
            "type": "Pair", "code": "K7QM-2WXR", "device_name": "Phone"
        }))
        .unwrap();
        assert!(matches!(pair, AuthRequest::Pair { .. }));
        let password: AuthRequest = serde_json::from_value(json!({
            "type": "Password", "user_name": "ada", "password": "x"
        }))
        .unwrap();
        assert_eq!(
            password,
            AuthRequest::Password {
                user_name: "ada".into(),
                password: "x".into(),
                totp_code: None,
                purpose: AuthPurpose::Login,
            }
        );
        let finish: AuthRequest = serde_json::from_value(json!({
            "type": "PasskeyFinish", "challenge_id": "c1", "credential": {"id": "abc"}
        }))
        .unwrap();
        assert!(matches!(finish, AuthRequest::PasskeyFinish { .. }));
        assert_eq!(
            serde_json::to_value(AuthResponse::Failed {
                reason: AuthFailure::RateLimited,
                retry_after_secs: Some(30),
            })
            .unwrap(),
            json!({"type": "Failed", "reason": "rate-limited", "retry_after_secs": 30})
        );
        assert_eq!(
            serde_json::to_value(AuthResponse::TotpRequired).unwrap(),
            json!({"type": "TotpRequired"})
        );
    }

    #[test]
    fn alerts_and_audit_use_wire_names() {
        let alert = RemoteAlert::Lockout {
            user_name: Some("ada".into()),
            ip: None,
            until: "2026-09-29T10:00:00Z".into(),
        };
        let value = serde_json::to_value(&alert).unwrap();
        assert_eq!(value["kind"], json!("lockout"));
        assert_eq!(serde_json::from_value::<RemoteAlert>(value).unwrap(), alert);
        assert_eq!(
            serde_json::to_value(RemoteAuditKind::PairingCreated).unwrap(),
            json!("pairing-created")
        );
        assert_eq!(
            serde_json::to_value(RemoteAuthMethod::PasswordTotp).unwrap(),
            json!("password-totp")
        );
        assert_eq!(
            serde_json::to_value(SessionEndReason::DeviceRevoked).unwrap(),
            json!("device-revoked")
        );
    }
}
