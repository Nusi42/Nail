//! The hop machinery the core's own outgoing requests share (ICS feeds,
//! [`crate::pim::feed_http`], and plug-in requests,
//! [`crate::plugin_http`]): `nail-proxy`'s pinned clients plus the core's
//! errors and the lookup rule (a DNS name is resolved once and **every**
//! answer must be public).

use std::net::IpAddr;

use nail_api::{ApiError, ErrorCode};
use nail_proxy::{Resolver, is_public_ip};
use url::{Host, Url};

pub(crate) fn network(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Network, message)
}

pub(crate) fn refused(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Forbidden, message)
}

pub(crate) fn is_loopback_host(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(name) => name.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(ip) => ip.is_loopback(),
        Host::Ipv6(ip) => ip.is_loopback(),
    }
}

/// The URL's credentials-free host, or why the hop is refused (`what`
/// names the URL kind in the message: "feed URLs", "URLs").
pub(crate) fn hop_host<'a>(url: &'a Url, what: &str) -> Result<Host<&'a str>, ApiError> {
    let Some(host) = url.host() else {
        return Err(refused(format!("{url} has no host")));
    };
    if !url.username().is_empty() || url.password().is_some() {
        return Err(refused(format!("{what} with credentials are refused")));
    }
    Ok(host)
}

/// Resolves `name` once; every answer must be public.
pub(crate) async fn resolve_public(
    resolver: &dyn Resolver,
    name: &str,
) -> Result<Vec<IpAddr>, ApiError> {
    let addrs = resolver
        .lookup(name)
        .await
        .map_err(|e| network(format!("cannot resolve {name}: {e}")))?;
    if addrs.is_empty() {
        return Err(network(format!("cannot resolve {name}")));
    }
    if let Some(bad) = addrs.iter().find(|ip| !is_public_ip(**ip)) {
        return Err(refused(format!(
            "{name} resolves to the private or local address {bad}"
        )));
    }
    Ok(addrs)
}

/// The response came from one of the checked addresses.
pub(crate) fn check_peer(response: &reqwest::Response, allowed: &[IpAddr]) -> Result<(), ApiError> {
    match nail_proxy::unchecked_peer(response, allowed) {
        Some(ip) => Err(refused(format!("connected to an unchecked address {ip}"))),
        None => Ok(()),
    }
}

/// The next hop of a redirect to `location` (fragment dropped); the error
/// message is `invalid`.
pub(crate) fn redirect_target(
    url: &Url,
    location: &reqwest::header::HeaderValue,
    invalid: &str,
) -> Result<Url, ApiError> {
    let location = location.to_str().map_err(|_| network(invalid))?;
    let mut next = url.join(location).map_err(|_| network(invalid))?;
    next.set_fragment(None);
    Ok(next)
}
