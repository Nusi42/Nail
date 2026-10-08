//! Configuration of the real-server interop tests (plan M2b WP44).
//!
//! The tests in `tests/servers.rs` and `tests/dav.rs` run only with
//! `NAIL_INTEROP=1` (the checks against the mock servers always run); without
//! it they return at once, so a plain `cargo test` passes anywhere. The CI
//! job "interop" (`.github/workflows/ci.yml`) starts Dovecot, Stalwart and
//! Radicale in containers and sets:
//!
//! * `NAIL_INTEROP_SERVERS`: `name=host:base[:feature…]`, comma-separated,
//!   e.g. `dovecot=localhost:10000,stalwart=localhost:20000:smtp`. A server
//!   listens on `base + 143` (IMAP, STARTTLS), `base + 993` (IMAPS),
//!   `base + 110` (POP3, STLS), `base + 995` (POP3S), with the `smtp`
//!   feature `base + 587` (submission, STARTTLS) and `base + 465`
//!   (submissions), and with the `sieve` feature `base + 4190`
//!   (ManageSieve, STARTTLS; M2c).
//! * `NAIL_INTEROP_CA`: PEM file of the CA that signed the servers'
//!   certificates (trusted as the only root).
//! * `NAIL_INTEROP_USER`, `NAIL_INTEROP_PASSWORD`: an account that exists on
//!   every server, with an email address as the user name.
//! * `NAIL_INTEROP_DAV` (optional, M3a WP72): CalDAV / CardDAV servers as
//!   `name=url`, comma-separated, e.g. `radicale=https://localhost:5232/`
//!   (the same user and CA). Unset = the DAV tests run against the mock
//!   server only.

use std::fmt;

use base64::Engine as _;

/// A server under test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    /// Label for messages ("dovecot").
    pub name: String,
    /// Host name, also the TLS server name.
    pub host: String,
    /// Port base (see the module docs).
    pub base: u16,
    /// Offers SMTP submission.
    pub smtp: bool,
    /// Offers ManageSieve (RFC 5804).
    pub sieve: bool,
}

impl Server {
    /// IMAP with STARTTLS.
    pub fn imap_port(&self) -> u16 {
        self.base + 143
    }
    /// IMAP over TLS.
    pub fn imaps_port(&self) -> u16 {
        self.base + 993
    }
    /// POP3 with STLS.
    pub fn pop3_port(&self) -> u16 {
        self.base + 110
    }
    /// POP3 over TLS.
    pub fn pop3s_port(&self) -> u16 {
        self.base + 995
    }
    /// Submission with STARTTLS.
    pub fn submission_port(&self) -> u16 {
        self.base + 587
    }
    /// Submission over TLS.
    pub fn submissions_port(&self) -> u16 {
        self.base + 465
    }
    /// ManageSieve with STARTTLS.
    pub fn sieve_port(&self) -> u16 {
        self.base + 4190
    }
}

/// A CalDAV / CardDAV server under test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DavServer {
    /// Label for messages ("radicale").
    pub name: String,
    /// The server URL discovery starts from.
    pub url: String,
}

/// Everything the tests need.
#[derive(Clone, PartialEq, Eq)]
pub struct Interop {
    /// The servers.
    pub servers: Vec<Server>,
    /// The CalDAV / CardDAV servers (`NAIL_INTEROP_DAV`).
    pub dav: Vec<DavServer>,
    /// DER certificates of the trusted CA.
    pub roots: Vec<Vec<u8>>,
    /// Account user name (an email address).
    pub user: String,
    /// Account password.
    pub password: String,
}

impl fmt::Debug for Interop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Interop")
            .field("servers", &self.servers)
            .field("dav", &self.dav)
            .field("roots", &self.roots.len())
            .field("user", &self.user)
            .finish_non_exhaustive()
    }
}

/// Parses `NAIL_INTEROP_SERVERS`.
pub fn parse_servers(spec: &str) -> Result<Vec<Server>, String> {
    let mut servers = Vec::new();
    for item in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (name, address) = item
            .split_once('=')
            .ok_or_else(|| format!("{item:?}: expected name=host:base"))?;
        let mut parts = address.split(':');
        let host = parts.next().unwrap_or_default().to_owned();
        let base: u16 = parts
            .next()
            .and_then(|p| p.parse().ok())
            .ok_or_else(|| format!("{item:?}: missing or invalid port base"))?;
        if host.is_empty() || name.is_empty() {
            return Err(format!("{item:?}: empty name or host"));
        }
        if base.checked_add(995).is_none() {
            return Err(format!("{item:?}: port base too high"));
        }
        let (mut smtp, mut sieve) = (false, false);
        for feature in parts {
            match feature {
                "smtp" => smtp = true,
                "sieve" => sieve = true,
                other => return Err(format!("{item:?}: unknown feature {other:?}")),
            }
        }
        if sieve && base.checked_add(4190).is_none() {
            return Err(format!("{item:?}: port base too high for ManageSieve"));
        }
        servers.push(Server {
            name: name.to_owned(),
            host,
            base,
            smtp,
            sieve,
        });
    }
    if servers.is_empty() {
        return Err("no servers".to_owned());
    }
    Ok(servers)
}

/// Parses `NAIL_INTEROP_DAV` (`name=url`, comma-separated; `https:` or
/// `http:` URLs).
pub fn parse_dav_servers(spec: &str) -> Result<Vec<DavServer>, String> {
    let mut out = Vec::new();
    for item in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (name, url) = item
            .split_once('=')
            .ok_or_else(|| format!("{item:?}: expected name=url"))?;
        let (name, url) = (name.trim(), url.trim());
        if name.is_empty() || !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(format!("{item:?}: expected name=https://host:port/"));
        }
        out.push(DavServer {
            name: name.to_owned(),
            url: url.to_owned(),
        });
    }
    Ok(out)
}

/// The DER certificates in a PEM text.
pub fn pem_certificates(pem: &str) -> Result<Vec<Vec<u8>>, String> {
    const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
    const END: &str = "-----END CERTIFICATE-----";
    let mut certificates = Vec::new();
    let mut rest = pem;
    while let Some(start) = rest.find(BEGIN) {
        let after = &rest[start + BEGIN.len()..];
        let end = after.find(END).ok_or("unterminated certificate")?;
        let body: String = after[..end].split_whitespace().collect();
        let der = base64::engine::general_purpose::STANDARD
            .decode(body)
            .map_err(|e| format!("bad base64: {e}"))?;
        certificates.push(der);
        rest = &after[end + END.len()..];
    }
    if certificates.is_empty() {
        return Err("no certificate in the PEM file".to_owned());
    }
    Ok(certificates)
}

/// The configuration from the environment; `None` unless `NAIL_INTEROP=1`.
///
/// # Panics
///
/// Panics when `NAIL_INTEROP=1` but the configuration is incomplete: a
/// misconfigured CI job must fail, not pass silently.
pub fn from_env() -> Option<Interop> {
    if std::env::var("NAIL_INTEROP").ok().as_deref() != Some("1") {
        return None;
    }
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"));
    let servers = parse_servers(&var("NAIL_INTEROP_SERVERS"))
        .unwrap_or_else(|e| panic!("NAIL_INTEROP_SERVERS: {e}"));
    let ca_path = var("NAIL_INTEROP_CA");
    let pem = std::fs::read_to_string(&ca_path).unwrap_or_else(|e| panic!("{ca_path}: {e}"));
    let dav = match std::env::var("NAIL_INTEROP_DAV") {
        Ok(spec) => parse_dav_servers(&spec).unwrap_or_else(|e| panic!("NAIL_INTEROP_DAV: {e}")),
        Err(_) => Vec::new(),
    };
    Some(Interop {
        servers,
        dav,
        roots: pem_certificates(&pem).unwrap_or_else(|e| panic!("{ca_path}: {e}")),
        user: var("NAIL_INTEROP_USER"),
        password: var("NAIL_INTEROP_PASSWORD"),
    })
}

/// A unique token for message ids and folder names of one test run.
pub fn unique(label: &str) -> String {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("{label}-{}-{nanos}-{n}", std::process::id())
}

/// A small RFC 5322 message with the given Message-ID.
pub fn message(from: &str, to: &str, message_id: &str, subject: &str) -> Vec<u8> {
    format!(
        "From: Nail interop <{from}>\r\n\
         To: <{to}>\r\n\
         Subject: {subject}\r\n\
         Date: Tue, 29 Sep 2026 12:00:00 +0000\r\n\
         Message-ID: <{message_id}>\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         \r\n\
         Sent by Nail's interop tests.\r\n"
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servers_parse() {
        let servers = parse_servers(
            "dovecot=localhost:10000:sieve, stalwart=127.0.0.1:20000:smtp:sieve, plain=h:30000",
        )
        .unwrap();
        assert_eq!(
            servers,
            [
                Server {
                    name: "dovecot".into(),
                    host: "localhost".into(),
                    base: 10000,
                    smtp: false,
                    sieve: true,
                },
                Server {
                    name: "stalwart".into(),
                    host: "127.0.0.1".into(),
                    base: 20000,
                    smtp: true,
                    sieve: true,
                },
                Server {
                    name: "plain".into(),
                    host: "h".into(),
                    base: 30000,
                    smtp: false,
                    sieve: false,
                },
            ]
        );
        assert_eq!(servers[0].imaps_port(), 10993);
        assert_eq!(servers[0].sieve_port(), 14190);
        assert_eq!(servers[1].submission_port(), 20587);
        assert_eq!(servers[1].pop3_port(), 20110);
        assert_eq!(servers[1].sieve_port(), 24190);
    }

    #[test]
    fn bad_server_specs_are_errors() {
        for bad in [
            "",
            "dovecot",
            "dovecot=localhost",
            "dovecot=localhost:x",
            "=localhost:1",
            "d=:1",
            "d=h:65000",
            "d=h:1:pop5",
            "d=h:62000:sieve",
        ] {
            assert!(parse_servers(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn dav_servers_parse() {
        assert_eq!(
            parse_dav_servers(
                "radicale=https://localhost:5232/, other = http://127.0.0.1:8080/dav/"
            )
            .unwrap(),
            [
                DavServer {
                    name: "radicale".into(),
                    url: "https://localhost:5232/".into(),
                },
                DavServer {
                    name: "other".into(),
                    url: "http://127.0.0.1:8080/dav/".into(),
                },
            ]
        );
        assert_eq!(parse_dav_servers("").unwrap(), []);
        for bad in ["radicale", "=https://x/", "r=ftp://x/", "r=localhost:5232"] {
            assert!(parse_dav_servers(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn pem_certificates_decode() {
        let pem = "junk\n-----BEGIN CERTIFICATE-----\nAAEC\nAw==\n-----END CERTIFICATE-----\n\
                   -----BEGIN CERTIFICATE-----\nBAU=\n-----END CERTIFICATE-----\n";
        assert_eq!(
            pem_certificates(pem).unwrap(),
            [vec![0, 1, 2, 3], vec![4, 5]]
        );
        assert!(pem_certificates("nothing").is_err());
        assert!(pem_certificates("-----BEGIN CERTIFICATE-----\nAA").is_err());
        assert!(
            pem_certificates("-----BEGIN CERTIFICATE-----\n!!\n-----END CERTIFICATE-----").is_err()
        );
    }

    #[test]
    fn disabled_without_the_switch() {
        if std::env::var("NAIL_INTEROP").is_err() {
            assert_eq!(from_env(), None);
        }
    }

    #[test]
    fn messages_and_tokens() {
        let raw = String::from_utf8(message("a@x", "b@x", "id@nail", "Hi")).unwrap();
        assert!(raw.contains("Message-ID: <id@nail>\r\n"));
        assert!(raw.contains("\r\n\r\nSent by"));
        assert_ne!(unique("t"), unique("t"));
    }
}
