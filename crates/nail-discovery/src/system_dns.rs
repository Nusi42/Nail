//! [`SystemDns`]: SRV / MX lookups through the operating system's resolver.
//!
//! On Windows every query goes to the DNS Client service through
//! `DnsQuery_W` (dnsapi), so Nail owns no DNS socket at all. A resolver
//! with its own sockets (hickory) binds UDP to the unspecified address,
//! which makes Windows Defender Firewall ask to "allow this app" (ADR 0057).
//! Other platforms fall back to `hickory-resolver` with the system's
//! configuration.

use std::fmt;

use nail_api::engine::BoxFuture;

use crate::net::{DnsLookup, MxRecord, NetError, SrvRecord};

/// [`DnsLookup`] with the operating system's resolver (see the module
/// docs).
#[derive(Clone)]
pub struct SystemDns {
    #[cfg(not(windows))]
    fallback: crate::net::HickoryDns,
}

impl fmt::Debug for SystemDns {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SystemDns").finish_non_exhaustive()
    }
}

impl SystemDns {
    /// The system's resolver.
    pub fn new() -> Result<Self, NetError> {
        #[cfg(windows)]
        {
            Ok(Self {})
        }
        #[cfg(not(windows))]
        {
            Ok(Self {
                fallback: crate::net::HickoryDns::system()?,
            })
        }
    }
}

#[cfg(windows)]
impl DnsLookup for SystemDns {
    fn srv<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<SrvRecord>, NetError>> {
        Box::pin(async move {
            let answers = blocking_query(name, windows_dns::Kind::Srv).await?;
            Ok(answers
                .into_iter()
                .filter_map(|a| match a {
                    windows_dns::Answer::Srv(srv) => Some(srv),
                    windows_dns::Answer::Mx(_) => None,
                })
                .collect())
        })
    }

    fn mx<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, Result<Vec<MxRecord>, NetError>> {
        Box::pin(async move {
            let answers = blocking_query(domain, windows_dns::Kind::Mx).await?;
            Ok(answers
                .into_iter()
                .filter_map(|a| match a {
                    windows_dns::Answer::Mx(mx) => Some(mx),
                    windows_dns::Answer::Srv(_) => None,
                })
                .collect())
        })
    }
}

/// `DnsQuery_W` blocks; it runs on tokio's blocking pool.
#[cfg(windows)]
async fn blocking_query(
    name: &str,
    kind: windows_dns::Kind,
) -> Result<Vec<windows_dns::Answer>, NetError> {
    let name = name.to_owned();
    tokio::task::spawn_blocking(move || windows_dns::query(&name, kind))
        .await
        .map_err(|e| NetError(format!("DNS query task failed: {e}")))?
}

#[cfg(not(windows))]
impl DnsLookup for SystemDns {
    fn srv<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<SrvRecord>, NetError>> {
        self.fallback.srv(name)
    }

    fn mx<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, Result<Vec<MxRecord>, NetError>> {
        self.fallback.mx(domain)
    }
}

/// Host name as discovery stores it: no trailing dot, lower case.
#[cfg_attr(not(windows), allow(dead_code))]
fn host(name: &str) -> String {
    name.trim_end_matches('.').to_ascii_lowercase()
}

#[cfg(windows)]
mod windows_dns {
    //! `DnsQuery_W` and the record list it returns. Every `unsafe` block of
    //! the crate is in this module, each with its safety argument.
    #![allow(unsafe_code)]

    use windows::Win32::Foundation::{
        DNS_ERROR_RCODE_NAME_ERROR, DNS_ERROR_RECORD_DOES_NOT_EXIST, DNS_INFO_NO_RECORDS,
        ERROR_SUCCESS, WIN32_ERROR,
    };
    use windows::Win32::NetworkManagement::Dns::{
        DNS_QUERY_NO_MULTICAST, DNS_QUERY_NO_NETBT, DNS_RECORDA, DNS_RECORDW, DNS_TYPE,
        DNS_TYPE_MX, DNS_TYPE_SRV, DnsFree, DnsFreeRecordList, DnsQuery_W,
    };
    use windows::core::{HSTRING, PWSTR};

    use super::host;
    use crate::net::{MxRecord, NetError, SrvRecord};

    /// The record type asked for.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum Kind {
        Srv,
        Mx,
    }

    impl Kind {
        fn dns_type(self) -> DNS_TYPE {
            match self {
                Self::Srv => DNS_TYPE_SRV,
                Self::Mx => DNS_TYPE_MX,
            }
        }
    }

    /// One answer record.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(super) enum Answer {
        Srv(SrvRecord),
        Mx(MxRecord),
    }

    /// `DNS_RECORD_FLAGS.Section` (the two lowest bits): authority and
    /// additional records are not answers.
    const SECTION_MASK: u32 = 0b11;
    const SECTION_AUTHORITY: u32 = 2;
    const SECTION_ADDITIONAL: u32 = 3;

    /// Whether a `DnsQuery_W` status means "no such records" (an empty
    /// answer, not an error).
    pub(super) fn is_no_records(status: WIN32_ERROR) -> bool {
        status == DNS_ERROR_RCODE_NAME_ERROR
            || status == DNS_ERROR_RECORD_DOES_NOT_EXIST
            || status.0 == DNS_INFO_NO_RECORDS.cast_unsigned()
    }

    /// The record list `DnsQuery_W` returned; freed when dropped.
    struct RecordList(*mut DNS_RECORDW);

    impl Drop for RecordList {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the pointer came from a successful DnsQuery_W and
                // is freed exactly once, with the matching free type.
                unsafe { DnsFree(Some(self.0.cast_const().cast()), DnsFreeRecordList) };
            }
        }
    }

    /// Asks the DNS Client service for `kind` records of `name` (as a
    /// fully qualified name; no LLMNR / NetBIOS).
    pub(super) fn query(name: &str, kind: Kind) -> Result<Vec<Answer>, NetError> {
        let fqdn = HSTRING::from(format!("{}.", name.trim_end_matches('.')));
        let mut results: *mut DNS_RECORDA = std::ptr::null_mut();
        // SAFETY: `fqdn` is a NUL-terminated UTF-16 string that outlives the
        // call; `results` is a valid out pointer; no extra / reserved args.
        let status = unsafe {
            DnsQuery_W(
                &fqdn,
                kind.dns_type(),
                DNS_QUERY_NO_MULTICAST | DNS_QUERY_NO_NETBT,
                None,
                &raw mut results,
                None,
            )
        };
        // DnsQuery_W fills a list of DNS_RECORDW (the A in the signature is
        // a metadata artefact; both share one layout).
        let list = RecordList(results.cast());
        if is_no_records(status) {
            return Ok(Vec::new());
        }
        if status != ERROR_SUCCESS {
            return Err(NetError(format!("DNS query failed (error {})", status.0)));
        }
        // SAFETY: `list` is the record list of a successful query, alive
        // until `list` is dropped below.
        let answers = unsafe { answers(list.0, kind) };
        drop(list);
        answers
    }

    /// Reads a record's name string.
    ///
    /// # Safety
    /// `text` is null or points to a NUL-terminated UTF-16 string.
    unsafe fn text(text: PWSTR) -> Result<String, NetError> {
        if text.is_null() {
            return Ok(String::new());
        }
        // SAFETY: non-null and NUL-terminated (caller's contract).
        unsafe { text.to_string() }.map_err(|e| NetError(format!("bad DNS name: {e}")))
    }

    /// The `kind` answers of a record list (other types, e.g. a CNAME on
    /// the way, and authority / additional records are skipped).
    ///
    /// # Safety
    /// `first` is null or the head of a valid `DNS_RECORDW` list whose
    /// `pNext` links and strings stay valid for the call.
    pub(super) unsafe fn answers(
        first: *const DNS_RECORDW,
        kind: Kind,
    ) -> Result<Vec<Answer>, NetError> {
        let wanted = kind.dns_type().0;
        let mut out = Vec::new();
        let mut next = first;
        while !next.is_null() {
            // SAFETY: non-null element of a valid list (caller's contract).
            let record = unsafe { &*next };
            next = record.pNext.cast_const();
            // SAFETY: `Flags` is a union of a u32 and a u32 bit field.
            let section = unsafe { record.Flags.DW } & SECTION_MASK;
            if record.wType != wanted
                || section == SECTION_AUTHORITY
                || section == SECTION_ADDITIONAL
            {
                continue;
            }
            match kind {
                Kind::Srv => {
                    // SAFETY: wType is SRV, so `Data` holds DNS_SRV_DATAW.
                    let srv = unsafe { record.Data.SRV };
                    out.push(Answer::Srv(SrvRecord {
                        priority: srv.wPriority,
                        weight: srv.wWeight,
                        port: srv.wPort,
                        // SAFETY: a record string of the list.
                        target: host(&unsafe { text(srv.pNameTarget) }?),
                    }));
                }
                Kind::Mx => {
                    // SAFETY: wType is MX, so `Data` holds DNS_MX_DATAW.
                    let mx = unsafe { record.Data.MX };
                    out.push(Answer::Mx(MxRecord {
                        preference: mx.wPreference,
                        // SAFETY: a record string of the list.
                        exchange: host(&unsafe { text(mx.pNameExchange) }?),
                    }));
                }
            }
        }
        Ok(out)
    }

    #[cfg(test)]
    mod tests {
        use windows::Win32::NetworkManagement::Dns::{
            DNS_MX_DATAW, DNS_RECORDW_0, DNS_RECORDW_1, DNS_SRV_DATAW, DNS_TYPE_CNAME,
        };

        use super::*;

        fn wide(text: &str) -> Vec<u16> {
            text.encode_utf16().chain(std::iter::once(0)).collect()
        }

        fn record(kind: DNS_TYPE, section: u32, data: DNS_RECORDW_1) -> DNS_RECORDW {
            DNS_RECORDW {
                wType: kind.0,
                Flags: DNS_RECORDW_0 { DW: section },
                Data: data,
                ..DNS_RECORDW::default()
            }
        }

        /// Links `records` in order and reads them back.
        fn read(records: &mut [DNS_RECORDW], kind: Kind) -> Vec<Answer> {
            for i in 1..records.len() {
                let next: *mut DNS_RECORDW = &raw mut records[i];
                records[i - 1].pNext = next;
            }
            // An empty list is a null head (an empty slice's pointer dangles).
            let head = records.first().map_or(std::ptr::null(), std::ptr::from_ref);
            // SAFETY: null or a well-formed list over `records` and its
            // strings, all alive for the call.
            unsafe { answers(head, kind) }.unwrap()
        }

        #[test]
        fn reads_srv_answers_and_skips_other_records() {
            let target = wide("IMAP.Example.org.");
            let none = wide(".");
            let alias = wide("alias.example.org");
            let srv = |priority, port, name: &Vec<u16>| DNS_RECORDW_1 {
                SRV: DNS_SRV_DATAW {
                    pNameTarget: PWSTR(name.as_ptr().cast_mut()),
                    wPriority: priority,
                    wWeight: 5,
                    wPort: port,
                    Pad: 0,
                },
            };
            let cname = DNS_RECORDW_1 {
                SRV: DNS_SRV_DATAW {
                    pNameTarget: PWSTR(alias.as_ptr().cast_mut()),
                    ..DNS_SRV_DATAW::default()
                },
            };
            let mut records = [
                record(DNS_TYPE_CNAME, 1, cname),
                record(DNS_TYPE_SRV, 1, srv(10, 993, &target)),
                record(DNS_TYPE_SRV, 1, srv(0, 0, &none)),
                record(DNS_TYPE_SRV, SECTION_ADDITIONAL, srv(1, 143, &target)),
            ];
            assert_eq!(
                read(&mut records, Kind::Srv),
                vec![
                    Answer::Srv(SrvRecord {
                        priority: 10,
                        weight: 5,
                        port: 993,
                        target: "imap.example.org".into(),
                    }),
                    Answer::Srv(SrvRecord {
                        priority: 0,
                        weight: 5,
                        port: 0,
                        target: String::new(),
                    }),
                ]
            );
        }

        #[test]
        fn reads_mx_answers() {
            let exchange = wide("mx1.Example.org");
            let mx = DNS_RECORDW_1 {
                MX: DNS_MX_DATAW {
                    pNameExchange: PWSTR(exchange.as_ptr().cast_mut()),
                    wPreference: 20,
                    Pad: 0,
                },
            };
            let mut records = [
                record(DNS_TYPE_MX, SECTION_AUTHORITY, mx),
                record(DNS_TYPE_MX, 1, mx),
            ];
            assert_eq!(
                read(&mut records, Kind::Mx),
                vec![Answer::Mx(MxRecord {
                    preference: 20,
                    exchange: "mx1.example.org".into(),
                })]
            );
            assert!(read(&mut [], Kind::Mx).is_empty());
        }

        #[test]
        fn missing_names_and_records_are_empty_answers() {
            assert!(is_no_records(DNS_ERROR_RCODE_NAME_ERROR));
            assert!(is_no_records(DNS_ERROR_RECORD_DOES_NOT_EXIST));
            assert!(is_no_records(WIN32_ERROR(
                DNS_INFO_NO_RECORDS.cast_unsigned()
            )));
            assert!(!is_no_records(ERROR_SUCCESS));
            assert!(!is_no_records(WIN32_ERROR(1460)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_lose_the_root_dot_and_case() {
        assert_eq!(host("Mail.Example.ORG."), "mail.example.org");
        assert_eq!(host("."), "");
    }

    /// A real lookup through the OS resolver (network): run with
    /// `cargo test -p nail-discovery -- --ignored system_dns`.
    #[tokio::test]
    #[ignore = "needs the network"]
    async fn system_dns_answers_real_queries() {
        let dns = SystemDns::new().unwrap();
        let mx = dns.mx("gmail.com").await.unwrap();
        assert!(
            mx.iter().any(|m| m.exchange.ends_with("google.com")),
            "{mx:?}"
        );
        let srv = dns.srv("_imaps._tcp.gmail.com").await.unwrap();
        assert!(srv.iter().any(|s| s.port == 993), "{srv:?}");
        assert!(
            dns.srv("_imaps._tcp.nail-does-not-exist.invalid")
                .await
                .unwrap()
                .is_empty()
        );
    }
}
