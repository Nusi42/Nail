//! The remote-content proxy (plan M2b WP42; contract ADR 0036).
//!
//! **Stub (WP40).** This crate is WP42's slot. It compiles, declares its
//! dependencies and checks them with smoke tests (`tests/libraries.rs`).
//!
//! # Expected shape
//!
//! A library over [`nail_api::remote_content`]; it does **not** depend on
//! `nail-core` or `nail-server`. The core uses it to sign URLs when it
//! renders an allowed message; `nail-server` (web mode, `GET`
//! [`nail_api::remote_content::PROXY_PATH`]`<token>`) and the desktop shell
//! (a custom protocol, wired by WP45) use it to serve them.
//!
//! * **Signed tokens** (`nail-proxy:<token>` in `sanitized_html`): HMAC-SHA-256
//!   over the original URL, the message id and an expiry, with a random
//!   per-process key; nothing else can mint one, so the proxy is never
//!   open. A token names only a URL that the message itself contains.
//! * **Rewriting**: the sanitizer hook in `nail-mime`
//!   (`src/remote_content.rs`, also WP42's) finds remote `src` /
//!   `background` / CSS `url(...)` references, counts them
//!   ([`nail_api::remote_content::RemoteContentInfo::resources`]) and, when
//!   the message is allowed, replaces them with signed proxy URLs.
//! * **Fetching**: `http` / `https` only; the resolver refuses loopback,
//!   private (RFC 1918, ULA), link-local, CGNAT, multicast, unspecified,
//!   documentation and IPv4-mapped forms of those, and the connection is
//!   made to the checked address (no second lookup → no DNS rebinding);
//!   redirects are followed only after the same checks, a few hops at
//!   most; no cookies, no `Referer`, a fixed `User-Agent`; size limit,
//!   timeout, and an allow-list of content types (images, fonts, CSS).
//! * **Responses**: `Content-Type` from the allow-list, `nosniff`, a
//!   restrictive `Content-Security-Policy`, cache headers; errors are
//!   typed so the server maps them to status codes.
//!
//! Tests include SSRF attempts (literal private IPs, decimal / octal /
//! IPv6-mapped forms, a host resolving to 127.0.0.1, DNS rebinding between
//! check and connect, redirects into private ranges) against local mock
//! servers only.
