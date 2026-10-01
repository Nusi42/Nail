//! Service discovery (RFC 6764, RFC 5397, RFC 4791 §6.2.1, RFC 6352 §7.1.1).
//!
//! Candidates, in order:
//!
//! 1. the configured URL (a context path, principal, home set or
//!    collection), then `/.well-known/caldav|carddav` on its host;
//! 2. for an email domain: `_caldavs._tcp` / `_carddavs._tcp` SRV records
//!    through the OS resolver ([`nail_discovery::DnsLookup`]; Nail opens
//!    no DNS sockets, ADR 0057), each target's `/.well-known/` URI and
//!    its root; TXT `path=` records are not read (the resolver has no TXT
//!    query, ADR 0058), the `/.well-known/` redirect covers them;
//! 3. `https://<domain>/.well-known/caldav|carddav`;
//! 4. Google's fixed endpoints for `gmail.com` / `googlemail.com`.
//!
//! Plain-HTTP SRV services (`_caldav._tcp`) are never used. On each
//! candidate the client asks for `current-user-principal`, then the
//! principal's `calendar-home-set` / `addressbook-home-set`, then lists
//! the home sets' collections. A refused login stops discovery at once.

use nail_discovery::DnsLookup;
use url::Url;

use crate::client::{DavClient, Depth, parse_server_url};
use crate::error::DavError;
use crate::multistatus::MsResponse;
use crate::ops::{CollectionKind, DavCollection, collection_from};
use crate::{NS_CALDAV, NS_CARDDAV, NS_DAV};

/// Google's CalDAV endpoint (OAuth2 bearer only).
pub const GOOGLE_CALDAV_URL: &str = "https://apidata.googleusercontent.com/caldav/v2/";
/// Google's CardDAV endpoint (OAuth2 bearer only).
pub const GOOGLE_CARDDAV_URL: &str = "https://www.googleapis.com/.well-known/carddav";

/// CalDAV or CardDAV.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DavService {
    /// Calendars and task lists.
    CalDav,
    /// Address books.
    CardDav,
}

impl DavService {
    /// The TLS SRV service label (`_caldavs._tcp`).
    pub fn srv_label(self) -> &'static str {
        match self {
            Self::CalDav => "_caldavs._tcp",
            Self::CardDav => "_carddavs._tcp",
        }
    }

    /// The `/.well-known/` path.
    pub fn well_known(self) -> &'static str {
        match self {
            Self::CalDav => "/.well-known/caldav",
            Self::CardDav => "/.well-known/carddav",
        }
    }

    /// The collection kind it serves.
    pub fn kind(self) -> CollectionKind {
        match self {
            Self::CalDav => CollectionKind::Calendar,
            Self::CardDav => CollectionKind::AddressBook,
        }
    }

    fn home_set(self) -> (&'static str, &'static str) {
        match self {
            Self::CalDav => (NS_CALDAV, "calendar-home-set"),
            Self::CardDav => (NS_CARDDAV, "addressbook-home-set"),
        }
    }

    /// The fixed endpoint of a well-known provider for `domain`.
    pub fn provider_url(self, domain: &str) -> Option<&'static str> {
        match domain.to_ascii_lowercase().as_str() {
            "gmail.com" | "googlemail.com" => Some(match self {
                Self::CalDav => GOOGLE_CALDAV_URL,
                Self::CardDav => GOOGLE_CARDDAV_URL,
            }),
            _ => None,
        }
    }
}

/// What to discover from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryInput {
    /// CalDAV or CardDAV.
    pub service: DavService,
    /// A configured URL, if any.
    pub url: Option<String>,
    /// The email address (or bare domain) whose domain is searched when
    /// there is no URL or it fails.
    pub email: Option<String>,
}

/// What discovery found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    /// The URL that led to the principal (to save in the configuration).
    pub context_url: Url,
    /// The principal.
    pub principal: Url,
    /// The home sets.
    pub home_sets: Vec<Url>,
    /// The collections of the service's kind in the home sets (or the
    /// configured collection itself).
    pub collections: Vec<DavCollection>,
}

/// The domain of an email address or a bare domain.
pub fn domain_of(email: &str) -> Option<String> {
    let domain = email.rsplit_once('@').map_or(email, |(_, d)| d).trim();
    let domain = domain.trim_end_matches('.').to_ascii_lowercase();
    (!domain.is_empty() && !domain.contains(['/', ':', ' ']) && domain.contains('.'))
        .then_some(domain)
}

/// The candidate URLs in order (see the module docs), without duplicates.
pub async fn candidates(dns: &dyn DnsLookup, input: &DiscoveryInput) -> Result<Vec<Url>, DavError> {
    let mut out: Vec<Url> = Vec::new();
    let mut push = |url: Url| {
        if !out.contains(&url) {
            out.push(url);
        }
    };
    if let Some(text) = input.url.as_deref().filter(|u| !u.trim().is_empty()) {
        let url = parse_server_url(text)?;
        push(url.clone());
        if let Ok(well_known) = url.join(input.service.well_known()) {
            push(well_known);
        }
    }
    if let Some(domain) = input.email.as_deref().and_then(domain_of) {
        let name = format!("{}.{domain}", input.service.srv_label());
        match dns.srv(&name).await {
            Ok(mut records) => {
                records.retain(|r| !r.target.is_empty() && r.target != ".");
                records.sort_by(|a, b| a.priority.cmp(&b.priority).then(b.weight.cmp(&a.weight)));
                for record in records {
                    let host = record.target.trim_end_matches('.');
                    let base = if record.port == 443 {
                        format!("https://{host}")
                    } else {
                        format!("https://{host}:{}", record.port)
                    };
                    if let Ok(url) = Url::parse(&format!("{base}{}", input.service.well_known())) {
                        push(url);
                    }
                    if let Ok(url) = Url::parse(&format!("{base}/")) {
                        push(url);
                    }
                }
            }
            Err(e) => tracing::debug!(%name, error = %e, "no SRV records"),
        }
        if let Ok(url) = Url::parse(&format!("https://{domain}{}", input.service.well_known())) {
            push(url);
        }
        if let Some(fixed) = input.service.provider_url(&domain)
            && let Ok(url) = Url::parse(fixed)
        {
            push(url);
        }
    }
    if out.is_empty() {
        return Err(DavError::NoService(
            "neither a URL nor an email domain".to_owned(),
        ));
    }
    Ok(out)
}

/// Discovers the principal, home sets and collections.
pub async fn discover(
    client: &DavClient,
    dns: &dyn DnsLookup,
    input: &DiscoveryInput,
) -> Result<Discovery, DavError> {
    let candidates = candidates(dns, input).await?;
    let mut last: Option<DavError> = None;
    for candidate in &candidates {
        match try_candidate(client, input.service, candidate).await {
            Ok(Some(found)) => return Ok(found),
            Ok(None) => {}
            Err(e @ (DavError::AuthFailed | DavError::Token(_))) => return Err(e),
            Err(e) => {
                tracing::debug!(%candidate, error = %e, "DAV discovery candidate failed");
                // Keep the most telling error: TLS beats network beats the rest.
                let rank = |e: &DavError| match e {
                    DavError::RedirectNeedsLogin(_) => 4,
                    DavError::Tls(_) => 3,
                    DavError::Network(_) | DavError::Timeout => 2,
                    DavError::RateLimited(_) => 1,
                    _ => 0,
                };
                if last.as_ref().is_none_or(|l| rank(&e) >= rank(l)) {
                    last = Some(e);
                }
            }
        }
    }
    match last {
        Some(
            e @ (DavError::Tls(_)
            | DavError::RedirectNeedsLogin(_)
            | DavError::Network(_)
            | DavError::Timeout
            | DavError::RateLimited(_)),
        ) => Err(e),
        _ => Err(DavError::NoService(
            candidates.first().map(Url::to_string).unwrap_or_default(),
        )),
    }
}

const PRINCIPAL_PROPS: &[(&str, &str)] = &[
    (NS_DAV, "current-user-principal"),
    (NS_DAV, "principal-URL"),
    (NS_DAV, "resourcetype"),
    (NS_CALDAV, "calendar-home-set"),
    (NS_CARDDAV, "addressbook-home-set"),
];

fn first_href(resp: &MsResponse, base: &Url, ns: &str, name: &str) -> Option<Url> {
    resp.prop_hrefs(base, ns, name).into_iter().next()
}

async fn try_candidate(
    client: &DavClient,
    service: DavService,
    candidate: &Url,
) -> Result<Option<Discovery>, DavError> {
    let (base, ms) = match client
        .propfind(candidate, Depth::Zero, PRINCIPAL_PROPS)
        .await
    {
        Ok(found) => found,
        Err(DavError::NotFound(_) | DavError::Forbidden(_) | DavError::Status { .. }) => {
            return Ok(None);
        }
        Err(DavError::Protocol(e)) => {
            tracing::debug!(%candidate, error = %e, "not a WebDAV answer");
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    let Some(resp) = ms.responses.first() else {
        return Ok(None);
    };
    let (home_ns, home_name) = service.home_set();
    // The candidate is itself a collection of the right kind.
    if let Some(collection) = collection_from(&base, resp).filter(|c| c.kind == service.kind()) {
        let collection = client
            .collection(&collection.url)
            .await
            .unwrap_or(collection);
        let principal = first_href(resp, &base, NS_DAV, "current-user-principal")
            .unwrap_or_else(|| collection.url.clone());
        return Ok(Some(Discovery {
            context_url: base.clone(),
            principal,
            home_sets: Vec::new(),
            collections: vec![collection],
        }));
    }
    let principal = first_href(resp, &base, NS_DAV, "current-user-principal")
        .or_else(|| first_href(resp, &base, NS_DAV, "principal-URL"));
    let (principal, homes) = match principal {
        Some(principal)
            if principal.as_str().trim_end_matches('/') != base.as_str().trim_end_matches('/') =>
        {
            let (pbase, pms) = client
                .propfind(&principal, Depth::Zero, PRINCIPAL_PROPS)
                .await?;
            let homes = pms
                .responses
                .first()
                .map(|r| r.prop_hrefs(&pbase, home_ns, home_name))
                .unwrap_or_default();
            (principal, homes)
        }
        Some(principal) => {
            let homes = resp.prop_hrefs(&base, home_ns, home_name);
            (principal, homes)
        }
        None => {
            let homes = resp.prop_hrefs(&base, home_ns, home_name);
            if homes.is_empty() {
                return Ok(None);
            }
            (base.clone(), homes)
        }
    };
    let homes = if homes.is_empty() {
        // Some servers keep collections right under the principal.
        vec![principal.clone()]
    } else {
        homes
    };
    let mut collections = Vec::new();
    for home in &homes {
        match client.collections(home, service.kind()).await {
            Ok(found) => {
                for collection in found {
                    if !collections
                        .iter()
                        .any(|c: &DavCollection| c.url == collection.url)
                    {
                        collections.push(collection);
                    }
                }
            }
            Err(DavError::NotFound(_) | DavError::Forbidden(_)) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(Some(Discovery {
        context_url: base,
        principal,
        home_sets: homes,
        collections,
    }))
}

/// Lists the home sets' collections again (after the first discovery).
pub async fn list_collections(
    client: &DavClient,
    service: DavService,
    home_sets: &[Url],
) -> Result<Vec<DavCollection>, DavError> {
    let mut out: Vec<DavCollection> = Vec::new();
    for home in home_sets {
        for collection in client.collections(home, service.kind()).await? {
            if !out.iter().any(|c| c.url == collection.url) {
                out.push(collection);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domains() {
        assert_eq!(domain_of("ada@Example.ORG").as_deref(), Some("example.org"));
        assert_eq!(domain_of("example.org.").as_deref(), Some("example.org"));
        assert_eq!(domain_of("ada@localhost"), None);
        assert_eq!(domain_of("ada@"), None);
        assert_eq!(domain_of("a@b/c.d"), None);
        assert_eq!(DavService::CalDav.srv_label(), "_caldavs._tcp");
        assert_eq!(DavService::CardDav.well_known(), "/.well-known/carddav");
        assert_eq!(
            DavService::CardDav.provider_url("GMail.com"),
            Some(GOOGLE_CARDDAV_URL)
        );
        assert_eq!(DavService::CalDav.provider_url("example.org"), None);
    }
}
