//! Small shared helpers without dependencies (wave Q2, SSOT / ODR / DRY,
//! ADR 0171): code that several crates had copied, defined once.
//!
//! - [`batch`]: batched writes with a one-by-one fallback.
//! - [`expiring`]: a thread-safe map whose entries expire (unlock caches).
//! - [`ical_text`]: iCalendar UTC stamps and durations (RFC 5545 texts).
//! - [`markup`]: HTML escaping.
//! - [`mime_text`]: byte search, header unfolding, backslash escapes,
//!   `Bcc` headers of raw messages.
//! - [`named`]: case-insensitive lookups in (name, value) pair lists.
//! - [`percent`]: percent-decoding of URL parts.
//! - [`redact`]: the stand-in text of a secret.
//! - [`sieve_lex`]: the lexer of Sieve scripts.
//! - [`time`]: proleptic Gregorian calendar arithmetic (Howard Hinnant's
//!   civil-date algorithms), the current Unix time, RFC 3339 / ISO dates.
//!
//! A helper belongs here when crates on both sides of the dependency graph
//! (the engines and the core) need it and it needs no dependency; anything
//! with network types goes to `nail-net`, contract types stay in `nail-api`.
//! Q2 packages add their own module files (one `pub mod` line each in this
//! file).

pub mod batch;
pub mod expiring;
pub mod ical_text;
pub mod markup;
pub mod mime_text;
pub mod named;
pub mod percent;
pub mod redact;
pub mod sieve_lex;
pub mod time;
