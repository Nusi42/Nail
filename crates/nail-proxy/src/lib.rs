//! The remote-content proxy (plan M2b WP42; contract ADR 0036; design
//! ADR 0038).
//!
//! Remote content in mail (images, fonts, CSS) is blocked by default. Once
//! the user allows it for a message, sender or domain, the reader loads it
//! **through this proxy**, never directly: the sender never sees the
//! user's address, and the proxy refuses anything that is not a public
//! internet address (no SSRF).
//!
//! A library over [`nail_api::remote_content`]; it does not depend on
//! `nail-core` or `nail-server`:
//!
//! * [`Signer`] (with the per-install [`ProxyKey`]): the core mints a
//!   token for each remote URL it finds in a message it renders
//!   ([`Signer::rewriter`] for the sanitizer hook). Tokens are
//!   HMAC-SHA-256 over purpose, message id, URL and expiry
//!   ([`token`]). Nothing else can mint one, so the proxy only ever
//!   fetches URLs taken from messages — it is **never an open proxy**.
//!   Render tokens ([`Purpose::Render`]) name a message whose HTML the
//!   sandbox origin may serve.
//! * [`policy`]: the URL rules (http/https, no credentials, no local names,
//!   no bad ports) and the address rules (public unicast only; IPv4-mapped,
//!   NAT64 and 6to4 forms are unwrapped and checked).
//! * [`Fetcher`]: DNS answers are checked before connecting and the
//!   connection is pinned to them (no DNS rebinding); redirects are hops
//!   checked the same way; no cookies, credentials, `Referer` or system
//!   proxy; size, type and time limits.
//! * [`Proxy`]: verify → optional [`TargetCheck`] by the core → fetch;
//!   errors map to HTTP statuses ([`ProxyError::status`]), and
//!   [`RESPONSE_HEADERS`] make every answer inert (`nosniff`, a sandbox
//!   CSP).

#![warn(missing_docs)]

pub mod fetch;
mod key;
pub mod policy;
mod service;
#[cfg(feature = "test-support")]
pub mod testing;
pub mod token;

pub use fetch::{FetchError, FetchLimits, Fetched, Fetcher, Resolver, SystemResolver};
pub use key::{KEY_LEN, KEY_SECRET_REF, KeyError, ProxyKey};
pub use policy::{UrlRefusal, is_public_ip};
pub use service::{Check, Proxy, ProxyError, ProxyResponse, RESPONSE_HEADERS, TargetCheck};
pub use token::{Grant, Purpose, SignError, Signer, TokenError};
